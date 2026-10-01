import type { ILogger as ExternalLogger } from "./interfaces";

export interface LoggerOptions {
  target: ExternalLogger;
}

export interface Logger {
  info(message: string): void;
}

class LoggerImplementation implements Logger {
  constructor(private readonly target: ExternalLogger) {}

  info(message: string): void {
    this.target.info(message);
  }
}

export const createLogger = (
  options: LoggerOptions
): Logger =>
  new LoggerImplementation(options.target);

