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
