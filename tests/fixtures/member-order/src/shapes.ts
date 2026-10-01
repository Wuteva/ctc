export abstract class OrderedShape {
  constructor(protected readonly size: number) {}

  public abstract area(): number;

  protected scale(): number {
    return this.size;
  }
}

export abstract class HelperFirst {
  protected helper(): void {}

  public run(): void {}
}

@Injectable()
export class Circle extends OrderedShape implements Named {
  constructor() {
    super(1);
  }

  public area(): number {
    return 3;
  }
}

export class Box<T> {
  private item?: T;

  public get(): T | undefined {
    return this.item;
  }
}
