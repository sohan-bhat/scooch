import { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import { Button } from '@/components/ui/button';
import content from '@/content.json';
import { actions } from './Live';
import { play } from './sound';
import { live } from './touch';

const words = content.story.desk;

function read() {
  return { lamp: live.lampTarget > 0.5, power: live.power, tape: live.tape };
}

export default function Controls() {
  const [state, setState] = useState(read);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    if (!open) return;
    const timer = window.setInterval(() => setState(read()), 300);
    return () => window.clearInterval(timer);
  }, [open]);

  const press = (name: 'lamp' | 'channel' | 'power' | 'tape' | 'ring' | 'sip' | 'staple', sound: string) => () => {
    live.lastScroll = -1e9;
    actions()[name]();
    play(sound);
    setState(read());
  };

  const list = [
    { key: 'lamp', label: state.lamp ? words.lampOff : words.lampOn, run: press('lamp', 'lamp') },
    { key: 'channel', label: words.channel, run: press('channel', 'channel'), off: !state.power },
    { key: 'power', label: state.power ? words.tvOff : words.tvOn, run: press('power', 'power') },
    { key: 'tape', label: state.tape ? words.stop : words.play, run: press('tape', 'cassette') },
    { key: 'ring', label: words.ring, run: press('ring', 'clock') },
    { key: 'sip', label: words.sip, run: press('sip', 'glass') },
    { key: 'staple', label: words.staple, run: press('staple', 'stapler') },
  ];

  const host = document.getElementById('desk-controls');
  if (!host) return null;
  return createPortal(
    <div
      role="group"
      aria-label={words.label}
      onFocus={() => {
        setOpen(true);
        setState(read());
      }}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setOpen(false);
      }}
      className="desk-controls page-gutter absolute inset-x-0 top-20 z-20 flex flex-wrap justify-center gap-2 tall:top-18"
      data-open={open ? '' : undefined}
    >
      {list.map((item) => (
        <Button key={item.key} type="button" variant="quiet" size="sm" disabled={item.off} onClick={item.run}>
          {item.label}
        </Button>
      ))}
    </div>,
    host,
  );
}
