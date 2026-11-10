export function load(path: string): string {
  try {
    return readFile(path);
  } catch (e) {
    throw new Error(`cannot load ${path}`, { cause: e });
  }
}

export function total(items: number[]): number {
  // reduce over an empty array needs the 0 seed
  const sum = items.reduce((a, b) => a + b, 0);
  const tax = sum * 0.2;
  return sum + tax;
}

export function warn(): void {
  try {
    run();
  } catch (e) {
    /* keep going */ report(e); /* logged */
  }
}

export const codes = (xs: number[]): string =>
  Array.from(xs, (b) => String.fromCharCode(b)).join("");
