package dev.pdbbranch;

public record RemoteCloneResult(
        String cloneMode,
        boolean snapshotCopyRequested,
        boolean snapshotCopyFellBack,
        String fallbackWarning
) {
}
