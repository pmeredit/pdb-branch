package dev.pdbbranch;

import java.time.OffsetDateTime;

public record BranchOptions(
        String fromPdb,
        boolean snapshotCopy,
        boolean openBranch,
        String profileName,
        OffsetDateTime expiresAt,
        String notes
) {
    public static BranchOptions defaults() {
        return new BranchOptions("GOLDEN_MASTER", true, true, null, null, null);
    }

    public BranchOptions withFromPdb(String value) {
        return new BranchOptions(value, snapshotCopy, openBranch, profileName, expiresAt, notes);
    }

    public BranchOptions withNotes(String value) {
        return new BranchOptions(fromPdb, snapshotCopy, openBranch, profileName, expiresAt, value);
    }
}
