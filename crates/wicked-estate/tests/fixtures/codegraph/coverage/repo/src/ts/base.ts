export interface Named {
  name(): string;
}
export abstract class Base {
  abstract area(): number;
  describe(): string {
    return "base";
  }
}
export function log(target: unknown): void {
  void target;
}
