export function load(path) {
  try {
    return read(path);
  } catch (e) {
    throw new Error(`cannot load ${path}`, { cause: e });
  }
}

export const codes = (xs) => Array.from(xs, (b) => String.fromCharCode(b)).join("");
