export function Hello(props: { name: string }) {
  try {
    track(props.name);
  } catch {}
  return <b>{props.name}</b>;
}
