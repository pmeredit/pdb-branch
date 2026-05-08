DECLARE
    v_count NUMBER;
BEGIN
    SELECT COUNT(*)
      INTO v_count
      FROM user_tables
     WHERE table_name = 'PDB_BRANCH_BRANCHES';

    IF v_count = 0 THEN
        EXECUTE IMMEDIATE q'[
            CREATE TABLE pdb_branch_branches (
                branch_name       VARCHAR2(128) PRIMARY KEY,
                parent_pdb        VARCHAR2(128),
                status            VARCHAR2(30) NOT NULL,
                profile_name      VARCHAR2(128),
                created_at        TIMESTAMP WITH TIME ZONE DEFAULT SYSTIMESTAMP NOT NULL,
                opened_at         TIMESTAMP WITH TIME ZONE,
                closed_at         TIMESTAMP WITH TIME ZONE,
                dropped_at        TIMESTAMP WITH TIME ZONE,
                last_activity_at  TIMESTAMP WITH TIME ZONE,
                expires_at        TIMESTAMP WITH TIME ZONE,
                score             NUMBER,
                notes             CLOB
            )
        ]';
    END IF;
END;
/

DECLARE
    v_count NUMBER;
BEGIN
    SELECT COUNT(*)
      INTO v_count
      FROM user_tables
     WHERE table_name = 'PDB_BRANCH_EVENTS';

    IF v_count = 0 THEN
        EXECUTE IMMEDIATE q'[
            CREATE TABLE pdb_branch_events (
                event_id          NUMBER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
                branch_name       VARCHAR2(128) NOT NULL,
                event_type        VARCHAR2(64) NOT NULL,
                event_at          TIMESTAMP WITH TIME ZONE DEFAULT SYSTIMESTAMP NOT NULL,
                details           CLOB
            )
        ]';
    END IF;
END;
/

DECLARE
    v_count NUMBER;
BEGIN
    SELECT COUNT(*)
      INTO v_count
      FROM user_tables
     WHERE table_name = 'PDB_BRANCH_PROFILES';

    IF v_count = 0 THEN
        EXECUTE IMMEDIATE q'[
            CREATE TABLE pdb_branch_profiles (
                profile_name           VARCHAR2(128) PRIMARY KEY,
                shares                 NUMBER,
                utilization_limit      NUMBER,
                parallel_server_limit  NUMBER,
                memory_min             NUMBER,
                memory_limit           NUMBER,
                description            VARCHAR2(4000),
                updated_at             TIMESTAMP WITH TIME ZONE DEFAULT SYSTIMESTAMP NOT NULL
            )
        ]';
    END IF;
END;
/

MERGE INTO pdb_branch_profiles t
USING (
    SELECT 'PDB_BRANCH_ACTIVE' profile_name, 8 shares, 80 utilization_limit,
           80 parallel_server_limit, 0 memory_min, 100 memory_limit,
           'Hot branch currently being exercised by agents' description
      FROM dual
    UNION ALL
    SELECT 'PDB_BRANCH_IDLE', 2, 25, 25, 0, 100,
           'Open branch expected to receive intermittent work'
      FROM dual
    UNION ALL
    SELECT 'PDB_BRANCH_BACKGROUND', 1, 5, 5, 0, 100,
           'Low-priority branch or branch waiting to be closed'
      FROM dual
) s
ON (t.profile_name = s.profile_name)
WHEN NOT MATCHED THEN
    INSERT (
        profile_name,
        shares,
        utilization_limit,
        parallel_server_limit,
        memory_min,
        memory_limit,
        description
    )
    VALUES (
        s.profile_name,
        s.shares,
        s.utilization_limit,
        s.parallel_server_limit,
        s.memory_min,
        s.memory_limit,
        s.description
    )
/
