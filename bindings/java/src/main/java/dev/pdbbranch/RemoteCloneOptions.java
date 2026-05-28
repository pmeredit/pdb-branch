package dev.pdbbranch;

import java.time.OffsetDateTime;

public record RemoteCloneOptions(
        String sourcePdb,
        String sourceDbLink,
        String cloneMode,
        boolean openBranch,
        String profileName,
        OffsetDateTime expiresAt,
        String notes,
        String createFileDest
) {
    public static RemoteCloneOptions defaults() {
        return new RemoteCloneOptions(
                "GOLDEN_MASTER",
                "PDB_BRANCH_SOURCE",
                "FULL",
                true,
                null,
                null,
                null,
                null
        );
    }

    public RemoteCloneOptions withSourcePdb(String value) {
        return new RemoteCloneOptions(
                value,
                sourceDbLink,
                cloneMode,
                openBranch,
                profileName,
                expiresAt,
                notes,
                createFileDest
        );
    }

    public RemoteCloneOptions withSourceDbLink(String value) {
        return new RemoteCloneOptions(
                sourcePdb,
                value,
                cloneMode,
                openBranch,
                profileName,
                expiresAt,
                notes,
                createFileDest
        );
    }

    public RemoteCloneOptions withCloneMode(String value) {
        return new RemoteCloneOptions(
                sourcePdb,
                sourceDbLink,
                value,
                openBranch,
                profileName,
                expiresAt,
                notes,
                createFileDest
        );
    }

    public RemoteCloneOptions withOpenBranch(boolean value) {
        return new RemoteCloneOptions(
                sourcePdb,
                sourceDbLink,
                cloneMode,
                value,
                profileName,
                expiresAt,
                notes,
                createFileDest
        );
    }

    public RemoteCloneOptions withProfileName(String value) {
        return new RemoteCloneOptions(
                sourcePdb,
                sourceDbLink,
                cloneMode,
                openBranch,
                value,
                expiresAt,
                notes,
                createFileDest
        );
    }

    public RemoteCloneOptions withExpiresAt(OffsetDateTime value) {
        return new RemoteCloneOptions(
                sourcePdb,
                sourceDbLink,
                cloneMode,
                openBranch,
                profileName,
                value,
                notes,
                createFileDest
        );
    }

    public RemoteCloneOptions withNotes(String value) {
        return new RemoteCloneOptions(
                sourcePdb,
                sourceDbLink,
                cloneMode,
                openBranch,
                profileName,
                expiresAt,
                value,
                createFileDest
        );
    }

    public RemoteCloneOptions withCreateFileDest(String value) {
        return new RemoteCloneOptions(
                sourcePdb,
                sourceDbLink,
                cloneMode,
                openBranch,
                profileName,
                expiresAt,
                notes,
                value
        );
    }
}
