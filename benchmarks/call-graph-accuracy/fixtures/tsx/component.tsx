// Call-graph accuracy fixture (TSX with JSX).
// Patterns:
//   - direct call:                  prepare(props)
//   - JSX component:                <Card />, <Button>...</Button>
//   - JSX namespaced component:     <Layout.Header />
//   - function reference (v1.11.1+): onClick={handleClick}, useEffect(loadData)

function prepare(props: { id: string }): { id: string } {
  return props;
}

function handleClick(): void {
  return undefined;
}

function loadData(): void {
  return undefined;
}

function Card(props: { id: string }): JSX.Element {
  return <div>{props.id}</div>;
}

function Button(props: { onClick: () => void }): JSX.Element {
  return <button onClick={props.onClick}>x</button>;
}

export function App(props: { id: string }): JSX.Element {
  prepare(props);
  React.useEffect(loadData);
  return (
    <Layout.Header>
      <Card id={props.id} />
      <Button onClick={handleClick} />
    </Layout.Header>
  );
}
