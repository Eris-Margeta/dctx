import React, { useState } from 'react';
import { format } from './format.js';

/** Counter button. */
export function Counter({ start = 0, onChange }) {
  const [count, setCount] = useState(start);
  const bump = () => {
    setCount(count + 1);
    onChange?.(count + 1);
  };
  return (
    <button className="counter" onClick={bump} title={`Count ${count}`}>
      {count > 1 ? <strong>{format(count)}</strong> : <em>none / zero</em>}
      {items.map((item) => (
        <Item key={item.id} {...item} render={() => draw(item)} />
      ))}
    </button>
  );
}

export const List = ({ rows }) => <ul>{rows.map((row) => <li key={row}>{row}</li>)}</ul>;

export default function App() {
  return <>
    <Counter start={1} onChange={(n) => console.log(n)} />
    <List rows={[1, 2]} />
  </>;
}
