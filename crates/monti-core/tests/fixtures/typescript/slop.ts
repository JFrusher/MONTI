export function load(path: string): string | null {
  try {
    return readFile(path);
  } catch (e) {}
  return null;
}

export function save(path: string, data: string): void {
  try {
    writeFile(path, data);
  } catch (err) {
  }
}

export function setAge(user: { age: number }): void {
  // Set the age
  // to twenty
  user.age = 20;
}

export function fetchUser(id: string, opts: object) {
  return fetchUserById(id, opts);
}

export function build(): Widget {
  const w = new Widget();
  if (w != null) {
    w.init();
  }
  return w;
}

export class Person {
  age = 0;
  grow(): void {
    // set age
    this.age = 20;
  }
}

export function quiet(): void {
  try {
    run();
  } catch {
    // ignore
  }
}

export function parse(raw: string, strict: boolean): any {
  // @ts-ignore
  return JSON.parse(raw);
}
