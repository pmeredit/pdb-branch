CREATE OR REPLACE PACKAGE pdb_branch AUTHID DEFINER AS
    PROCEDURE create_branch(
        p_branch_name    IN VARCHAR2,
        p_from_pdb       IN VARCHAR2 DEFAULT 'GOLDEN_MASTER',
        p_snapshot_copy  IN VARCHAR2 DEFAULT 'Y',
        p_open           IN VARCHAR2 DEFAULT 'Y',
        p_profile_name   IN VARCHAR2 DEFAULT NULL,
        p_expires_at     IN TIMESTAMP WITH TIME ZONE DEFAULT NULL,
        p_notes          IN CLOB DEFAULT NULL
    );

    PROCEDURE open_branch(
        p_branch_name   IN VARCHAR2,
        p_profile_name  IN VARCHAR2 DEFAULT NULL
    );

    PROCEDURE close_branch(
        p_branch_name  IN VARCHAR2,
        p_immediate    IN VARCHAR2 DEFAULT 'Y'
    );

    PROCEDURE drop_branch(
        p_branch_name          IN VARCHAR2,
        p_including_datafiles  IN VARCHAR2 DEFAULT 'Y'
    );

    PROCEDURE set_profile(
        p_branch_name   IN VARCHAR2,
        p_profile_name  IN VARCHAR2,
        p_reopen        IN VARCHAR2 DEFAULT 'Y'
    );

    PROCEDURE record_activity(
        p_branch_name  IN VARCHAR2,
        p_status       IN VARCHAR2 DEFAULT NULL
    );

    PROCEDURE record_score(
        p_branch_name  IN VARCHAR2,
        p_score        IN NUMBER,
        p_notes        IN CLOB DEFAULT NULL
    );

    PROCEDURE promote_branch(
        p_branch_name  IN VARCHAR2,
        p_notes        IN CLOB DEFAULT NULL
    );

    PROCEDURE create_snapshot(
        p_pdb_name       IN VARCHAR2,
        p_snapshot_name  IN VARCHAR2 DEFAULT NULL
    );

    PROCEDURE drop_snapshot(
        p_pdb_name       IN VARCHAR2,
        p_snapshot_name  IN VARCHAR2
    );

    PROCEDURE cleanup(
        p_close_idle_after_minutes  IN NUMBER DEFAULT 60,
        p_drop_expired              IN VARCHAR2 DEFAULT 'Y'
    );

    PROCEDURE configure_resource_plan(
        p_plan_name  IN VARCHAR2 DEFAULT 'PDB_BRANCH_PLAN',
        p_activate   IN VARCHAR2 DEFAULT 'N'
    );
END pdb_branch;
/

CREATE OR REPLACE PACKAGE BODY pdb_branch AS
    c_status_created     CONSTANT VARCHAR2(30) := 'CREATED';
    c_status_open        CONSTANT VARCHAR2(30) := 'OPEN';
    c_status_closed      CONSTANT VARCHAR2(30) := 'CLOSED';
    c_status_dropped     CONSTANT VARCHAR2(30) := 'DROPPED';
    c_status_promoted    CONSTANT VARCHAR2(30) := 'PROMOTED';

    FUNCTION yes(p_value IN VARCHAR2) RETURN BOOLEAN IS
    BEGIN
        RETURN UPPER(NVL(TRIM(p_value), 'N')) IN ('Y', 'YES', 'TRUE', '1');
    END yes;

    FUNCTION clean_name(p_name IN VARCHAR2, p_kind IN VARCHAR2) RETURN VARCHAR2 IS
        v_name VARCHAR2(128) := UPPER(TRIM(p_name));
    BEGIN
        IF v_name IS NULL THEN
            RAISE_APPLICATION_ERROR(-20000, p_kind || ' is required');
        END IF;

        IF NOT REGEXP_LIKE(v_name, '^[A-Z][A-Z0-9_$#]{0,127}$') THEN
            RAISE_APPLICATION_ERROR(
                -20001,
                p_kind || ' must be an unquoted Oracle identifier using A-Z, 0-9, _, $, or #'
            );
        END IF;

        RETURN DBMS_ASSERT.SIMPLE_SQL_NAME(v_name);
    END clean_name;

    FUNCTION qname(p_name IN VARCHAR2, p_kind IN VARCHAR2) RETURN VARCHAR2 IS
    BEGIN
        RETURN DBMS_ASSERT.ENQUOTE_NAME(clean_name(p_name, p_kind), FALSE);
    END qname;

    FUNCTION qliteral(p_value IN VARCHAR2) RETURN VARCHAR2 IS
    BEGIN
        RETURN DBMS_ASSERT.ENQUOTE_LITERAL(p_value);
    END qliteral;

    PROCEDURE log_event(
        p_branch_name IN VARCHAR2,
        p_event_type  IN VARCHAR2,
        p_details     IN CLOB DEFAULT NULL
    ) IS
    BEGIN
        INSERT INTO pdb_branch_events(branch_name, event_type, details)
        VALUES (clean_name(p_branch_name, 'branch name'), UPPER(p_event_type), p_details);
    END log_event;

    PROCEDURE upsert_branch(
        p_branch_name      IN VARCHAR2,
        p_parent_pdb       IN VARCHAR2,
        p_status           IN VARCHAR2,
        p_profile_name     IN VARCHAR2 DEFAULT NULL,
        p_expires_at       IN TIMESTAMP WITH TIME ZONE DEFAULT NULL,
        p_notes            IN CLOB DEFAULT NULL
    ) IS
        v_branch_name  VARCHAR2(128) := clean_name(p_branch_name, 'branch name');
        v_parent_pdb   VARCHAR2(128) := clean_name(p_parent_pdb, 'parent PDB');
        v_profile_name VARCHAR2(128);
    BEGIN
        IF p_profile_name IS NOT NULL THEN
            v_profile_name := clean_name(p_profile_name, 'profile name');
        END IF;

        MERGE INTO pdb_branch_branches t
        USING (
            SELECT v_branch_name branch_name,
                   v_parent_pdb parent_pdb,
                   UPPER(p_status) status,
                   v_profile_name profile_name,
                   p_expires_at expires_at,
                   p_notes notes
              FROM dual
        ) s
        ON (t.branch_name = s.branch_name)
        WHEN MATCHED THEN UPDATE
             SET t.parent_pdb = NVL(t.parent_pdb, s.parent_pdb),
                 t.status = s.status,
                 t.profile_name = COALESCE(s.profile_name, t.profile_name),
                 t.expires_at = COALESCE(s.expires_at, t.expires_at),
                 t.last_activity_at = SYSTIMESTAMP,
                 t.notes = CASE
                     WHEN s.notes IS NULL THEN t.notes
                     WHEN t.notes IS NULL THEN s.notes
                     ELSE t.notes || CHR(10) || s.notes
                 END
        WHEN NOT MATCHED THEN
             INSERT (
                 branch_name,
                 parent_pdb,
                 status,
                 profile_name,
                 created_at,
                 last_activity_at,
                 expires_at,
                 notes
             )
             VALUES (
                 s.branch_name,
                 s.parent_pdb,
                 s.status,
                 s.profile_name,
                 SYSTIMESTAMP,
                 SYSTIMESTAMP,
                 s.expires_at,
                 s.notes
             );
    END upsert_branch;

    PROCEDURE create_branch(
        p_branch_name    IN VARCHAR2,
        p_from_pdb       IN VARCHAR2 DEFAULT 'GOLDEN_MASTER',
        p_snapshot_copy  IN VARCHAR2 DEFAULT 'Y',
        p_open           IN VARCHAR2 DEFAULT 'Y',
        p_profile_name   IN VARCHAR2 DEFAULT NULL,
        p_expires_at     IN TIMESTAMP WITH TIME ZONE DEFAULT NULL,
        p_notes          IN CLOB DEFAULT NULL
    ) IS
        v_branch_name VARCHAR2(128) := clean_name(p_branch_name, 'branch name');
        v_from_pdb    VARCHAR2(128) := clean_name(p_from_pdb, 'parent PDB');
        v_sql         VARCHAR2(32767);
    BEGIN
        v_sql :=
            'CREATE PLUGGABLE DATABASE ' || qname(v_branch_name, 'branch name') ||
            ' FROM ' || qname(v_from_pdb, 'parent PDB');

        IF yes(p_snapshot_copy) THEN
            v_sql := v_sql || ' SNAPSHOT COPY';
        END IF;

        EXECUTE IMMEDIATE v_sql;

        upsert_branch(
            p_branch_name  => v_branch_name,
            p_parent_pdb   => v_from_pdb,
            p_status       => c_status_created,
            p_profile_name => p_profile_name,
            p_expires_at   => p_expires_at,
            p_notes        => p_notes
        );
        log_event(v_branch_name, 'CREATE_BRANCH', v_sql);
        COMMIT;

        IF yes(p_open) THEN
            open_branch(v_branch_name, p_profile_name);
        END IF;
    END create_branch;

    PROCEDURE open_branch(
        p_branch_name   IN VARCHAR2,
        p_profile_name  IN VARCHAR2 DEFAULT NULL
    ) IS
        v_branch_name VARCHAR2(128) := clean_name(p_branch_name, 'branch name');
    BEGIN
        EXECUTE IMMEDIATE 'ALTER PLUGGABLE DATABASE ' || qname(v_branch_name, 'branch name') || ' OPEN';

        UPDATE pdb_branch_branches
           SET status = c_status_open,
               opened_at = SYSTIMESTAMP,
               closed_at = NULL,
               last_activity_at = SYSTIMESTAMP
         WHERE branch_name = v_branch_name;

        log_event(v_branch_name, 'OPEN_BRANCH');
        COMMIT;

        IF p_profile_name IS NOT NULL THEN
            set_profile(v_branch_name, p_profile_name, 'Y');
        END IF;
    END open_branch;

    PROCEDURE close_branch(
        p_branch_name  IN VARCHAR2,
        p_immediate    IN VARCHAR2 DEFAULT 'Y'
    ) IS
        v_branch_name VARCHAR2(128) := clean_name(p_branch_name, 'branch name');
        v_sql         VARCHAR2(32767);
    BEGIN
        v_sql := 'ALTER PLUGGABLE DATABASE ' || qname(v_branch_name, 'branch name') || ' CLOSE';
        IF yes(p_immediate) THEN
            v_sql := v_sql || ' IMMEDIATE';
        END IF;

        EXECUTE IMMEDIATE v_sql;

        UPDATE pdb_branch_branches
           SET status = c_status_closed,
               closed_at = SYSTIMESTAMP,
               last_activity_at = SYSTIMESTAMP
         WHERE branch_name = v_branch_name;

        log_event(v_branch_name, 'CLOSE_BRANCH', v_sql);
        COMMIT;
    END close_branch;

    PROCEDURE drop_branch(
        p_branch_name          IN VARCHAR2,
        p_including_datafiles  IN VARCHAR2 DEFAULT 'Y'
    ) IS
        v_branch_name VARCHAR2(128) := clean_name(p_branch_name, 'branch name');
        v_sql         VARCHAR2(32767);
    BEGIN
        BEGIN
            EXECUTE IMMEDIATE
                'ALTER PLUGGABLE DATABASE ' || qname(v_branch_name, 'branch name') ||
                ' CLOSE IMMEDIATE';
        EXCEPTION
            WHEN OTHERS THEN
                NULL;
        END;

        v_sql := 'DROP PLUGGABLE DATABASE ' || qname(v_branch_name, 'branch name');
        IF yes(p_including_datafiles) THEN
            v_sql := v_sql || ' INCLUDING DATAFILES';
        ELSE
            v_sql := v_sql || ' KEEP DATAFILES';
        END IF;

        EXECUTE IMMEDIATE v_sql;

        UPDATE pdb_branch_branches
           SET status = c_status_dropped,
               dropped_at = SYSTIMESTAMP,
               last_activity_at = SYSTIMESTAMP
         WHERE branch_name = v_branch_name;

        log_event(v_branch_name, 'DROP_BRANCH', v_sql);
        COMMIT;
    END drop_branch;

    PROCEDURE set_profile(
        p_branch_name   IN VARCHAR2,
        p_profile_name  IN VARCHAR2,
        p_reopen        IN VARCHAR2 DEFAULT 'Y'
    ) IS
        v_branch_name   VARCHAR2(128) := clean_name(p_branch_name, 'branch name');
        v_profile_name  VARCHAR2(128) := clean_name(p_profile_name, 'profile name');
        v_original_con  VARCHAR2(128) := SYS_CONTEXT('USERENV', 'CON_NAME');
    BEGIN
        IF LENGTH(v_profile_name) > 30 THEN
            RAISE_APPLICATION_ERROR(-20002, 'profile name must be 30 characters or fewer');
        END IF;

        EXECUTE IMMEDIATE 'ALTER SESSION SET CONTAINER = ' || qname(v_branch_name, 'branch name');
        EXECUTE IMMEDIATE
            'ALTER SYSTEM SET DB_PERFORMANCE_PROFILE = ' ||
            qliteral(v_profile_name) ||
            ' SCOPE = SPFILE';
        EXECUTE IMMEDIATE 'ALTER SESSION SET CONTAINER = ' || qname(v_original_con, 'container name');

        UPDATE pdb_branch_branches
           SET profile_name = v_profile_name,
               last_activity_at = SYSTIMESTAMP
         WHERE branch_name = v_branch_name;

        log_event(v_branch_name, 'SET_PROFILE', v_profile_name);
        COMMIT;

        IF yes(p_reopen) THEN
            BEGIN
                EXECUTE IMMEDIATE
                    'ALTER PLUGGABLE DATABASE ' ||
                    qname(v_branch_name, 'branch name') ||
                    ' CLOSE IMMEDIATE';
            EXCEPTION
                WHEN OTHERS THEN
                    NULL;
            END;
            EXECUTE IMMEDIATE
                'ALTER PLUGGABLE DATABASE ' ||
                qname(v_branch_name, 'branch name') ||
                ' OPEN';

            UPDATE pdb_branch_branches
               SET status = c_status_open,
                   opened_at = SYSTIMESTAMP,
                   closed_at = NULL,
                   last_activity_at = SYSTIMESTAMP
             WHERE branch_name = v_branch_name;
            COMMIT;
        END IF;
    EXCEPTION
        WHEN OTHERS THEN
            BEGIN
                EXECUTE IMMEDIATE 'ALTER SESSION SET CONTAINER = ' || qname(v_original_con, 'container name');
            EXCEPTION
                WHEN OTHERS THEN
                    NULL;
            END;
            RAISE;
    END set_profile;

    PROCEDURE record_activity(
        p_branch_name  IN VARCHAR2,
        p_status       IN VARCHAR2 DEFAULT NULL
    ) IS
        v_branch_name VARCHAR2(128) := clean_name(p_branch_name, 'branch name');
    BEGIN
        UPDATE pdb_branch_branches
           SET status = COALESCE(UPPER(TRIM(p_status)), status),
               last_activity_at = SYSTIMESTAMP
         WHERE branch_name = v_branch_name;

        log_event(v_branch_name, 'RECORD_ACTIVITY', p_status);
        COMMIT;
    END record_activity;

    PROCEDURE record_score(
        p_branch_name  IN VARCHAR2,
        p_score        IN NUMBER,
        p_notes        IN CLOB DEFAULT NULL
    ) IS
        v_branch_name VARCHAR2(128) := clean_name(p_branch_name, 'branch name');
    BEGIN
        UPDATE pdb_branch_branches
           SET score = p_score,
               last_activity_at = SYSTIMESTAMP,
               notes = CASE
                   WHEN p_notes IS NULL THEN notes
                   WHEN notes IS NULL THEN p_notes
                   ELSE notes || CHR(10) || p_notes
               END
         WHERE branch_name = v_branch_name;

        log_event(v_branch_name, 'RECORD_SCORE', TO_CHAR(p_score));
        COMMIT;
    END record_score;

    PROCEDURE promote_branch(
        p_branch_name  IN VARCHAR2,
        p_notes        IN CLOB DEFAULT NULL
    ) IS
        v_branch_name VARCHAR2(128) := clean_name(p_branch_name, 'branch name');
    BEGIN
        UPDATE pdb_branch_branches
           SET status = c_status_promoted,
               last_activity_at = SYSTIMESTAMP,
               notes = CASE
                   WHEN p_notes IS NULL THEN notes
                   WHEN notes IS NULL THEN p_notes
                   ELSE notes || CHR(10) || p_notes
               END
         WHERE branch_name = v_branch_name;

        log_event(v_branch_name, 'PROMOTE_BRANCH', p_notes);
        COMMIT;
    END promote_branch;

    PROCEDURE create_snapshot(
        p_pdb_name       IN VARCHAR2,
        p_snapshot_name  IN VARCHAR2 DEFAULT NULL
    ) IS
        v_pdb_name      VARCHAR2(128) := clean_name(p_pdb_name, 'PDB name');
        v_snapshot_name VARCHAR2(128);
        v_original_con  VARCHAR2(128) := SYS_CONTEXT('USERENV', 'CON_NAME');
        v_sql           VARCHAR2(32767);
    BEGIN
        IF p_snapshot_name IS NOT NULL THEN
            v_snapshot_name := clean_name(p_snapshot_name, 'snapshot name');
        END IF;

        EXECUTE IMMEDIATE 'ALTER SESSION SET CONTAINER = ' || qname(v_pdb_name, 'PDB name');
        v_sql := 'ALTER PLUGGABLE DATABASE SNAPSHOT';
        IF v_snapshot_name IS NOT NULL THEN
            v_sql := v_sql || ' ' || qname(v_snapshot_name, 'snapshot name');
        END IF;
        EXECUTE IMMEDIATE v_sql;
        EXECUTE IMMEDIATE 'ALTER SESSION SET CONTAINER = ' || qname(v_original_con, 'container name');

        log_event(v_pdb_name, 'CREATE_SNAPSHOT', NVL(v_snapshot_name, '<system-generated>'));
        COMMIT;
    EXCEPTION
        WHEN OTHERS THEN
            BEGIN
                EXECUTE IMMEDIATE 'ALTER SESSION SET CONTAINER = ' || qname(v_original_con, 'container name');
            EXCEPTION
                WHEN OTHERS THEN
                    NULL;
            END;
            RAISE;
    END create_snapshot;

    PROCEDURE drop_snapshot(
        p_pdb_name       IN VARCHAR2,
        p_snapshot_name  IN VARCHAR2
    ) IS
        v_pdb_name      VARCHAR2(128) := clean_name(p_pdb_name, 'PDB name');
        v_snapshot_name VARCHAR2(128) := clean_name(p_snapshot_name, 'snapshot name');
        v_original_con  VARCHAR2(128) := SYS_CONTEXT('USERENV', 'CON_NAME');
    BEGIN
        EXECUTE IMMEDIATE 'ALTER SESSION SET CONTAINER = ' || qname(v_pdb_name, 'PDB name');
        EXECUTE IMMEDIATE
            'ALTER PLUGGABLE DATABASE DROP SNAPSHOT ' ||
            qname(v_snapshot_name, 'snapshot name');
        EXECUTE IMMEDIATE 'ALTER SESSION SET CONTAINER = ' || qname(v_original_con, 'container name');

        log_event(v_pdb_name, 'DROP_SNAPSHOT', v_snapshot_name);
        COMMIT;
    EXCEPTION
        WHEN OTHERS THEN
            BEGIN
                EXECUTE IMMEDIATE 'ALTER SESSION SET CONTAINER = ' || qname(v_original_con, 'container name');
            EXCEPTION
                WHEN OTHERS THEN
                    NULL;
            END;
            RAISE;
    END drop_snapshot;

    PROCEDURE cleanup(
        p_close_idle_after_minutes  IN NUMBER DEFAULT 60,
        p_drop_expired              IN VARCHAR2 DEFAULT 'Y'
    ) IS
    BEGIN
        IF yes(p_drop_expired) THEN
            FOR r IN (
                SELECT branch_name
                  FROM pdb_branch_branches
                 WHERE status NOT IN (c_status_promoted, c_status_dropped)
                   AND expires_at IS NOT NULL
                   AND expires_at < SYSTIMESTAMP
            ) LOOP
                drop_branch(r.branch_name, 'Y');
            END LOOP;
        END IF;

        IF p_close_idle_after_minutes IS NOT NULL THEN
            FOR r IN (
                SELECT branch_name
                  FROM pdb_branch_branches
                 WHERE status = c_status_open
                   AND last_activity_at < SYSTIMESTAMP - NUMTODSINTERVAL(p_close_idle_after_minutes, 'MINUTE')
            ) LOOP
                close_branch(r.branch_name, 'Y');
            END LOOP;
        END IF;
    END cleanup;

    PROCEDURE configure_resource_plan(
        p_plan_name  IN VARCHAR2 DEFAULT 'PDB_BRANCH_PLAN',
        p_activate   IN VARCHAR2 DEFAULT 'N'
    ) IS
        v_plan_name VARCHAR2(128) := clean_name(p_plan_name, 'resource plan name');
        v_exists    NUMBER;
    BEGIN
        EXECUTE IMMEDIATE 'BEGIN DBMS_RESOURCE_MANAGER.CREATE_PENDING_AREA(); END;';

        SELECT COUNT(*)
          INTO v_exists
          FROM dba_cdb_rsrc_plans
         WHERE plan = v_plan_name;

        IF v_exists = 0 THEN
            EXECUTE IMMEDIATE
                'BEGIN DBMS_RESOURCE_MANAGER.CREATE_CDB_PLAN(plan => :1, comment => :2); END;'
                USING v_plan_name, 'PDB branch resource plan';
        END IF;

        FOR r IN (
            SELECT profile_name,
                   shares,
                   utilization_limit,
                   parallel_server_limit,
                   memory_min,
                   memory_limit
              FROM pdb_branch_profiles
        ) LOOP
            IF LENGTH(r.profile_name) > 30 THEN
                RAISE_APPLICATION_ERROR(
                    -20003,
                    'profile name ' || r.profile_name || ' must be 30 characters or fewer'
                );
            END IF;

            BEGIN
                EXECUTE IMMEDIATE
                    'BEGIN DBMS_RESOURCE_MANAGER.UPDATE_CDB_PROFILE_DIRECTIVE(' ||
                    'plan => :1, profile => :2, new_shares => :3, ' ||
                    'new_utilization_limit => :4, new_parallel_server_limit => :5); END;'
                    USING
                        v_plan_name,
                        r.profile_name,
                        r.shares,
                        r.utilization_limit,
                        r.parallel_server_limit;
            EXCEPTION
                WHEN OTHERS THEN
                    EXECUTE IMMEDIATE
                        'BEGIN DBMS_RESOURCE_MANAGER.CREATE_CDB_PROFILE_DIRECTIVE(' ||
                        'plan => :1, profile => :2, shares => :3, utilization_limit => :4, ' ||
                        'parallel_server_limit => :5); END;'
                        USING
                            v_plan_name,
                            r.profile_name,
                            r.shares,
                            r.utilization_limit,
                            r.parallel_server_limit;
            END;
        END LOOP;

        EXECUTE IMMEDIATE 'BEGIN DBMS_RESOURCE_MANAGER.VALIDATE_PENDING_AREA(); END;';
        EXECUTE IMMEDIATE 'BEGIN DBMS_RESOURCE_MANAGER.SUBMIT_PENDING_AREA(); END;';

        IF yes(p_activate) THEN
            EXECUTE IMMEDIATE
                'ALTER SYSTEM SET RESOURCE_MANAGER_PLAN = ' ||
                qliteral(v_plan_name) ||
                ' SCOPE = BOTH';
        END IF;
    EXCEPTION
        WHEN OTHERS THEN
            BEGIN
                EXECUTE IMMEDIATE 'BEGIN DBMS_RESOURCE_MANAGER.CLEAR_PENDING_AREA(); END;';
            EXCEPTION
                WHEN OTHERS THEN
                    NULL;
            END;
            RAISE;
    END configure_resource_plan;
END pdb_branch;
/
