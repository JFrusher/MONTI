export function load(path) {
  try {
    return read(path);
  } catch (e) {
    /* nothing */
  }
}

export const view = (user) => render(user);

export const tag = <i>slop</i>;
