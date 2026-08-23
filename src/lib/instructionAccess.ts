import type { GlobalInstructionFile, GlobalInstructionSourceRow } from "../types";

export function canEditInstruction(file: GlobalInstructionFile): boolean {
  return file.editable;
}

export function canOpenInstruction(file: GlobalInstructionFile): boolean {
  return file.load_status !== "locally_invisible" && file.abs_path.length > 0;
}

export function showsLoadStatus(file: GlobalInstructionFile): boolean {
  return file.evidence !== "no_mechanism";
}

export function showsLoadBadge(file: GlobalInstructionFile): boolean {
  return showsLoadStatus(file) && file.load_status !== "loaded";
}

export function showsEvidenceBadge(file: GlobalInstructionFile): boolean {
  return file.evidence !== "verified";
}

export function isIdleSource(row: GlobalInstructionSourceRow): boolean {
  return row.files.length > 0 && row.files.every(isIdleFile);
}

export function idleSourceLabel(row: GlobalInstructionSourceRow): string {
  return row.files.some((file) => file.evidence === "no_mechanism") ? "无机制" : "未创建";
}

function isIdleFile(file: GlobalInstructionFile): boolean {
  return file.evidence === "no_mechanism" || file.load_status === "not_created";
}
