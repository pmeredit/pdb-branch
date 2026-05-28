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

    PROCEDURE clone_branch_from_remote(
        p_branch_name       IN VARCHAR2,
        p_source_pdb        IN VARCHAR2,
        p_source_db_link    IN VARCHAR2,
        p_clone_mode        IN VARCHAR2 DEFAULT 'FULL',
        p_open              IN VARCHAR2 DEFAULT 'Y',
        p_profile_name      IN VARCHAR2 DEFAULT NULL,
        p_expires_at        IN TIMESTAMP WITH TIME ZONE DEFAULT NULL,
        p_notes             IN CLOB DEFAULT NULL,
        p_create_file_dest  IN VARCHAR2 DEFAULT NULL
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

    FUNCTION db_link_name(p_name IN VARCHAR2) RETURN VARCHAR2 IS
        v_name VARCHAR2(128) := UPPER(TRIM(p_name));
    BEGIN
        IF v_name IS NULL THEN
            RAISE_APPLICATION_ERROR(-20008, 'database link name is required');
        END IF;

        IF NOT REGEXP_LIKE(v_name, '^[A-Z][A-Z0-9_$#]{0,127}(\.[A-Z][A-Z0-9_$#]{1,128})*$') THEN
            RAISE_APPLICATION_ERROR(
                -20009,
                'database link name must be an unquoted Oracle identifier or dotted identifier'
            );
        END IF;

        RETURN DBMS_ASSERT.QUALIFIED_SQL_NAME(v_name);
    END db_link_name;

    FUNCTION datafile_directory(p_pdb_name IN VARCHAR2) RETURN VARCHAR2 IS
        v_file_name VARCHAR2(4000);
        v_pos       PLS_INTEGER;
    BEGIN
        EXECUTE IMMEDIATE
            'SELECT MIN(file_name) FROM cdb_data_files WHERE con_id = ' ||
            '(SELECT con_id FROM v$pdbs WHERE name = :1)'
            INTO v_file_name
            USING p_pdb_name;

        IF v_file_name IS NULL THEN
            RAISE_APPLICATION_ERROR(-20004, 'no data files found for PDB ' || p_pdb_name);
        END IF;

        v_pos := GREATEST(INSTR(v_file_name, '/', -1), INSTR(v_file_name, '\', -1));
        IF v_pos = 0 THEN
            RAISE_APPLICATION_ERROR(-20005, 'unable to derive datafile directory for PDB ' || p_pdb_name);
        END IF;

        RETURN SUBSTR(v_file_name, 1, v_pos);
    END datafile_directory;

    FUNCTION parent_directory(p_directory IN VARCHAR2) RETURN VARCHAR2 IS
        v_directory VARCHAR2(4000) := RTRIM(p_directory, '/\');
        v_pos       PLS_INTEGER;
    BEGIN
        v_pos := GREATEST(INSTR(v_directory, '/', -1), INSTR(v_directory, '\', -1));
        IF v_pos = 0 THEN
            RAISE_APPLICATION_ERROR(-20006, 'unable to derive parent datafile directory');
        END IF;

        RETURN SUBSTR(v_directory, 1, v_pos);
    END parent_directory;

    FUNCTION create_file_dest(p_from_pdb IN VARCHAR2) RETURN VARCHAR2 IS
        v_destination VARCHAR2(4000);
    BEGIN
        EXECUTE IMMEDIATE
            'SELECT value FROM v$parameter WHERE name = ''db_create_file_dest'''
            INTO v_destination;

        IF v_destination IS NOT NULL THEN
            RETURN v_destination;
        END IF;

        RETURN parent_directory(datafile_directory(p_from_pdb));
    END create_file_dest;

    FUNCTION remote_create_file_dest(p_create_file_dest IN VARCHAR2) RETURN VARCHAR2 IS
        v_destination VARCHAR2(4000) := TRIM(p_create_file_dest);
    BEGIN
        IF v_destination IS NOT NULL THEN
            RETURN v_destination;
        END IF;

        EXECUTE IMMEDIATE
            'SELECT value FROM v$parameter WHERE name = ''db_create_file_dest'''
            INTO v_destination;

        IF v_destination IS NULL THEN
            RAISE_APPLICATION_ERROR(
                -20010,
                'remote PDB clone requires DB_CREATE_FILE_DEST or an explicit create file destination'
            );
        END IF;

        RETURN v_destination;
    END remote_create_file_dest;

    FUNCTION clean_clone_mode(p_clone_mode IN VARCHAR2) RETURN VARCHAR2 IS
        v_clone_mode VARCHAR2(30) := UPPER(TRIM(NVL(p_clone_mode, 'FULL')));
    BEGIN
        IF v_clone_mode NOT IN ('FULL', 'AUTO', 'SNAPSHOT') THEN
            RAISE_APPLICATION_ERROR(
                -20011,
                'clone mode must be FULL, AUTO, or SNAPSHOT'
            );
        END IF;

        RETURN v_clone_mode;
    END clean_clone_mode;

    FUNCTION database_banner RETURN VARCHAR2 IS
        v_banner VARCHAR2(4000);
    BEGIN
        EXECUTE IMMEDIATE 'SELECT banner FROM v$version WHERE ROWNUM = 1'
            INTO v_banner;
        RETURN v_banner;
    END database_banner;

    FUNCTION is_oracle_free RETURN BOOLEAN IS
    BEGIN
        RETURN INSTR(UPPER(NVL(database_banner, '')), 'FREE') > 0;
    END is_oracle_free;

    FUNCTION snapshot_copy_unsupported(
        p_error_code     IN NUMBER,
        p_error_message  IN VARCHAR2
    ) RETURN BOOLEAN IS
    BEGIN
        RETURN p_error_code IN (-17525, -65169) OR
               INSTR(p_error_message, 'ORA-17525') > 0 OR
               INSTR(p_error_message, 'ORA-65169') > 0;
    END snapshot_copy_unsupported;

    FUNCTION create_branch_sql(
        p_branch_name       IN VARCHAR2,
        p_from_pdb          IN VARCHAR2,
        p_create_file_dest  IN VARCHAR2,
        p_snapshot_copy     IN BOOLEAN
    ) RETURN VARCHAR2 IS
        v_sql VARCHAR2(32767);
    BEGIN
        v_sql :=
            'CREATE PLUGGABLE DATABASE ' || qname(p_branch_name, 'branch name') ||
            ' FROM ' || qname(p_from_pdb, 'parent PDB');

        IF p_snapshot_copy THEN
            v_sql := v_sql || ' SNAPSHOT COPY';
        END IF;

        RETURN
            v_sql ||
            ' CREATE_FILE_DEST = ' ||
            qliteral(p_create_file_dest);
    END create_branch_sql;

    FUNCTION clone_branch_from_remote_sql(
        p_branch_name       IN VARCHAR2,
        p_source_pdb        IN VARCHAR2,
        p_source_db_link    IN VARCHAR2,
        p_create_file_dest  IN VARCHAR2,
        p_snapshot_copy     IN BOOLEAN
    ) RETURN VARCHAR2 IS
        v_sql VARCHAR2(32767);
    BEGIN
        v_sql :=
            'CREATE PLUGGABLE DATABASE ' || qname(p_branch_name, 'branch name') ||
            ' FROM ' || qname(p_source_pdb, 'source PDB') ||
            '@' || db_link_name(p_source_db_link);

        IF p_snapshot_copy THEN
            v_sql := v_sql || ' SNAPSHOT COPY';
        END IF;

        RETURN
            v_sql ||
            ' CREATE_FILE_DEST = ' ||
            qliteral(p_create_file_dest);
    END clone_branch_from_remote_sql;

    PROCEDURE log_event(
        p_branch_name IN VARCHAR2,
        p_event_type  IN VARCHAR2,
        p_details     IN CLOB DEFAULT NULL
    ) IS
        v_branch_name VARCHAR2(128) := clean_name(p_branch_name, 'branch name');
    BEGIN
        INSERT INTO pdb_branch_events(branch_name, event_type, details)
        VALUES (v_branch_name, UPPER(p_event_type), p_details);
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
        v_branch_name       VARCHAR2(128) := clean_name(p_branch_name, 'branch name');
        v_from_pdb          VARCHAR2(128) := clean_name(p_from_pdb, 'parent PDB');
        v_create_file_dest  VARCHAR2(4000);
        v_requested_snapshot BOOLEAN := yes(p_snapshot_copy);
        v_used_snapshot      BOOLEAN;
        v_error_code         NUMBER;
        v_error_message      VARCHAR2(4000);
        v_fallback_warning   VARCHAR2(4000);
        v_sql               VARCHAR2(32767);
    BEGIN
        v_create_file_dest := create_file_dest(v_from_pdb);
        IF v_requested_snapshot AND is_oracle_free THEN
            v_used_snapshot := FALSE;
            v_fallback_warning :=
                'WARNING: SNAPSHOT COPY requested on Oracle Free; created with full clone ' ||
                'because Oracle Free container storage does not support storage snapshots';
        ELSE
            v_used_snapshot := v_requested_snapshot;
        END IF;

        v_sql := create_branch_sql(
            v_branch_name,
            v_from_pdb,
            v_create_file_dest,
            v_used_snapshot
        );

        BEGIN
            EXECUTE IMMEDIATE v_sql;
        EXCEPTION
            WHEN OTHERS THEN
                v_error_code := SQLCODE;
                v_error_message := SQLERRM;
                IF v_used_snapshot AND snapshot_copy_unsupported(v_error_code, v_error_message) THEN
                    v_used_snapshot := FALSE;
                    v_fallback_warning :=
                        'WARNING: SNAPSHOT COPY requested but Oracle reported storage snapshots ' ||
                        'are unsupported (' || TO_CHAR(v_error_code) || ': ' ||
                        SUBSTR(v_error_message, 1, 3500) || '); created with full clone';
                    v_sql := create_branch_sql(
                        v_branch_name,
                        v_from_pdb,
                        v_create_file_dest,
                        v_used_snapshot
                    );
                    EXECUTE IMMEDIATE v_sql;
                ELSE
                    RAISE;
                END IF;
        END;

        upsert_branch(
            p_branch_name  => v_branch_name,
            p_parent_pdb   => v_from_pdb,
            p_status       => c_status_created,
            p_profile_name => p_profile_name,
            p_expires_at   => p_expires_at,
            p_notes        => p_notes
        );
        log_event(v_branch_name, 'CREATE_BRANCH', v_sql);
        IF v_requested_snapshot AND NOT v_used_snapshot THEN
            log_event(v_branch_name, 'SNAPSHOT_COPY_FALLBACK', v_fallback_warning);
        END IF;
        COMMIT;

        IF yes(p_open) THEN
            open_branch(v_branch_name, p_profile_name);
        END IF;
    END create_branch;

    PROCEDURE clone_branch_from_remote(
        p_branch_name       IN VARCHAR2,
        p_source_pdb        IN VARCHAR2,
        p_source_db_link    IN VARCHAR2,
        p_clone_mode        IN VARCHAR2 DEFAULT 'FULL',
        p_open              IN VARCHAR2 DEFAULT 'Y',
        p_profile_name      IN VARCHAR2 DEFAULT NULL,
        p_expires_at        IN TIMESTAMP WITH TIME ZONE DEFAULT NULL,
        p_notes             IN CLOB DEFAULT NULL,
        p_create_file_dest  IN VARCHAR2 DEFAULT NULL
    ) IS
        v_branch_name       VARCHAR2(128) := clean_name(p_branch_name, 'branch name');
        v_source_pdb        VARCHAR2(128) := clean_name(p_source_pdb, 'source PDB');
        v_source_db_link    VARCHAR2(128) := db_link_name(p_source_db_link);
        v_create_file_dest  VARCHAR2(4000);
        v_clone_mode        VARCHAR2(30) := clean_clone_mode(p_clone_mode);
        v_used_snapshot     BOOLEAN;
        v_error_code        NUMBER;
        v_error_message     VARCHAR2(4000);
        v_fallback_warning  VARCHAR2(4000);
        v_sql               VARCHAR2(32767);
    BEGIN
        v_create_file_dest := remote_create_file_dest(p_create_file_dest);
        v_used_snapshot := v_clone_mode IN ('AUTO', 'SNAPSHOT');
        v_sql := clone_branch_from_remote_sql(
            v_branch_name,
            v_source_pdb,
            v_source_db_link,
            v_create_file_dest,
            v_used_snapshot
        );

        BEGIN
            EXECUTE IMMEDIATE v_sql;
        EXCEPTION
            WHEN OTHERS THEN
                v_error_code := SQLCODE;
                v_error_message := SQLERRM;
                IF v_clone_mode = 'AUTO' AND
                   v_used_snapshot AND
                   snapshot_copy_unsupported(v_error_code, v_error_message) THEN
                    v_used_snapshot := FALSE;
                    v_fallback_warning :=
                        'WARNING: remote SNAPSHOT COPY requested with clone mode AUTO but Oracle ' ||
                        'reported storage snapshots are unsupported (' ||
                        TO_CHAR(v_error_code) || ': ' ||
                        SUBSTR(v_error_message, 1, 3400) || '); pushed with full clone';
                    v_sql := clone_branch_from_remote_sql(
                        v_branch_name,
                        v_source_pdb,
                        v_source_db_link,
                        v_create_file_dest,
                        v_used_snapshot
                    );
                    EXECUTE IMMEDIATE v_sql;
                ELSE
                    RAISE;
                END IF;
        END;

        upsert_branch(
            p_branch_name  => v_branch_name,
            p_parent_pdb   => v_source_pdb,
            p_status       => c_status_created,
            p_profile_name => p_profile_name,
            p_expires_at   => p_expires_at,
            p_notes        => p_notes
        );
        log_event(v_branch_name, 'CLONE_BRANCH_FROM_REMOTE', v_sql);
        IF v_clone_mode = 'AUTO' AND NOT v_used_snapshot THEN
            log_event(v_branch_name, 'REMOTE_SNAPSHOT_COPY_FALLBACK', v_fallback_warning);
        END IF;
        COMMIT;

        IF yes(p_open) THEN
            open_branch(v_branch_name, p_profile_name);
        END IF;
    END clone_branch_from_remote;

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

        EXECUTE IMMEDIATE
            'SELECT COUNT(*) FROM dba_cdb_rsrc_plans WHERE plan = :1'
            INTO v_exists
            USING v_plan_name;

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
