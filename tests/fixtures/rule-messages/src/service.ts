type Result<T> = T | Error;

export function load(): Result<number> {
  try {
    return 1;
  } catch {
    return new Error("failed");
  }
}

export function save(): Result<number> {
  return;
}

export function fail(): void {
  throw new Error("failed");
}