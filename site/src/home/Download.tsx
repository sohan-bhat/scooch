import { DownloadSimple } from '@phosphor-icons/react/dist/ssr';
import { Button } from '@/components/ui/button';
import content from '@/content.json';
import { choose, click, usePlatform } from './platform';

type Props = {
  size?: 'sm' | 'md' | 'lg';
  className?: string;
  onPick?: () => void;
};

export function Download({ size = 'lg', className, onPick }: Props) {
  const { install } = content;
  const [{ arch, kind }] = usePlatform();
  const pick = () => {
    click();
    onPick?.();
  };
  return (
    <>
      {install.systems.map((item) => {
        const main = choose(item, arch, kind);
        return (
          <Button key={item.id} asChild size={size} className={className}>
            <a data-get={item.id} href={`${install.release}${main.file}`} download onClick={pick}>
              <DownloadSimple weight="bold" aria-hidden />
              {main.label}
            </a>
          </Button>
        );
      })}
      <Button asChild size={size} className={className}>
        <a data-get="any" href="#install" onClick={onPick}>
          <DownloadSimple weight="bold" aria-hidden />
          {install.action}
        </a>
      </Button>
    </>
  );
}
