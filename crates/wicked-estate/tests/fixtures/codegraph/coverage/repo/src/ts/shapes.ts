import { Base, Named, log } from "./base";
import "./theme.css";

export { log } from "./base";

@log
export class Square extends Base implements Named {
  side: number = 1;
  area(): number {
    return this.side * this.side;
  }
  override describe(): string {
    return "square";
  }
  name(): string {
    return "sq";
  }
}

export function grow(s: Square): Square {
  const n = new Square();
  n.side = s.side + 1;
  return n;
}
