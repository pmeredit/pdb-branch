package dev.pdbbranch;

public record ResourcePlanOptions(String planName, boolean activate) {
    public static ResourcePlanOptions defaults() {
        return new ResourcePlanOptions("PDB_BRANCH_PLAN", false);
    }
}
