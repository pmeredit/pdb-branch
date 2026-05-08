package dev.pdbbranch;

public record CleanupOptions(int closeIdleAfterMinutes, boolean dropExpired) {
    public static CleanupOptions defaults() {
        return new CleanupOptions(60, true);
    }
}
