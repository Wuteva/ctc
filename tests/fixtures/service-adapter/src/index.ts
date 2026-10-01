import { createServiceAdapter } from "./adapter";
import { createLogger } from "./logger";

const logger = createLogger({ target: console });
createServiceAdapter({ logger }).start();

