export class OrderedService {
  constructor(private readonly name: string) {}

  @Get()
  public run(): string {
    return this.name;
  }

  protected helper(): void {}

  #cache = new Map<string, string>();
}

export class Empty {}

export class PrivateFirst {
  private count = 0;

  run(): number {
    return this.count;
  }
}
