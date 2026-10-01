type Result<T> =
  | { ok: true; value: T }
  | { ok: false; error: Error };

declare const dangerous: {
  call(): number;
};

export function valid(flag: boolean): Result<number> {
  if (flag) {
    return { ok: true, value: 1 };
  }
  return { ok: false, error: new Error("invalid") };
}

export function validIfElse(flag: boolean): Result<number> {
  if (flag) {
    return { ok: true, value: 1 };
  } else {
    return { ok: false, error: new Error("invalid") };
  }
}

export function fallsThrough(flag: boolean): Result<number> {
  if (flag) {
    return { ok: true, value: 1 };
  }
}

export function bareReturn(): Result<number> {
  return;
}

export function throws(): Result<number> {
  throw new Error("failed");
}

export function conditionalThrow(flag: boolean): Result<number> {
  if (flag) {
    throw new Error("failed");
  }
  return { ok: true, value: 1 };
}

export function catches(): Result<number> {
  try {
    return { ok: true, value: dangerous.call() };
  } catch (error) {
    return { ok: false, error: error as Error };
  }
}

export async function rejects(): Promise<Result<number>> {
  return Promise.reject(new Error("failed"));
}

export function executorRejects(): Promise<Result<number>> {
  return new Promise((resolve, reject) => {
    reject(new Error("failed"));
  });
}

export function nestedExecutorRejects(): Promise<Result<number>> {
  return new Promise((resolve, reject) => {
    queueMicrotask(() => reject(new Error("failed")));
  });
}

export function commentedSources(): Promise<Result<number>> {
  dangerous /* comment */ .call();
  return Promise /* comment */ .reject(new Error("failed"));
}
