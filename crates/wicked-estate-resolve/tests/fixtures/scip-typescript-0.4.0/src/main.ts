import { helper, LIMIT, Greeter } from "./util";

export function run(): number {
  const g = new Greeter();
  g.greet("a");
  const f = helper;
  return helper(LIMIT) + f(1);
}
