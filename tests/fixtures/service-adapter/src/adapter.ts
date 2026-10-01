import { ILogger } from "./interfaces";

export interface ServiceAdapterOptions {
  logger: ILogger;
}

export interface ServiceAdapter {
  start(): void;
}

class ServiceAdapterImplementation implements ServiceAdapter {
  constructor(private readonly logger: ILogger) {}

  start(): void {
    this.logger.info("started");
  }
}

export const createServiceAdapter = (
  options: ServiceAdapterOptions
): ServiceAdapter =>
  new ServiceAdapterImplementation(options.logger);

