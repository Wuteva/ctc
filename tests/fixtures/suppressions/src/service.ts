export function first(): void {
  // ctc-ignore-next-line no-throw-ts -- legacy error path
  throw new Error("first");
}

export function second(): void {
  const text = "// ctc-ignore-next-line no-throw-ts";
  throw new Error(text);
}

export function third(): void {
  /* ctc-ignore-next-line no-try-ts, no-throw-ts */

  try {
    run();
  } catch {
    return;
  }
}

function run(): void {}
