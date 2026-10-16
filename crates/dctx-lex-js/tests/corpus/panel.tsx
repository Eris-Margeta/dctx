import { useMemo } from 'react';
import type { Theme } from './theme';

export interface PanelProps {
  title: string;
  theme?: Theme;
  onClose(): void;
}

/** Renders a panel. */
export const Panel = ({ title, theme, onClose }: PanelProps) => {
  const style = useMemo(() => ({ color: theme?.accent ?? 'black' }), [theme]);
  return (
    <section style={style} aria-label={title}>
      <h2>{title.toUpperCase()}</h2>
      <button onClick={() => onClose()}>close</button>
    </section>
  );
};

export function wrap<T,>(items: T[]): JSX.Element[] {
  return items.map((item, index) => <div key={index}>{String(item)}</div>);
}

export default class Board extends Component<PanelProps> {
  render(): JSX.Element {
    return <Panel {...this.props} />;
  }
}
