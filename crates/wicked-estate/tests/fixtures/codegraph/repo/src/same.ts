export class A { run(): number { return 1; } } export class B { run(): number { return 2; } }
export function both(): number { return new A().run() + new B().run(); }
