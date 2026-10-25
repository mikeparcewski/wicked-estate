// Install smoke fixture (wicked-estate#360).
export function tsLeaf(): number {
  return 1;
}

export function tsCaller(): number {
  return tsLeaf() + 1;
}
