package dev.pdbbranch;

public record BranchCreateResult(
        boolean snapshotCopyRequested,
        boolean snapshotCopyFellBack,
        String fallbackWarning
) {
}
