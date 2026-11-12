import { ArrowUpRight } from '@phosphor-icons/react/dist/ssr';
import { Footer } from '@/components/Footer';
import { Header } from '@/components/Header';
import content from '@/content.json';

export function SourcesPage() {
  const { sources } = content;
  return (
    <>
      <Header />
      <main className="page-gutter pt-36 pb-24 tall:pt-28">
        <h1 className="text-(length:--text-beat) text-paper">{sources.title}</h1>
        <p className="mt-5 max-w-[44ch] text-(length:--text-support) text-plastic">{sources.line}</p>
        <div className="mt-14 overflow-x-auto rounded-panel bg-room-2 shadow-[inset_0_0_0_1px_rgb(243_236_220/0.08)]">
          <table className="w-full min-w-[40rem] border-collapse text-left text-[0.9375rem]">
            <thead>
              <tr className="text-plastic">
                {sources.columns.map((column) => (
                  <th key={column} scope="col" className="px-5 pt-5 pb-3 font-semibold">
                    {column}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {sources.rows.map((row) => (
                <tr key={row.what} className="border-t border-paper/8">
                  <th scope="row" className="px-5 py-4 font-semibold text-paper">
                    <a href={row.url} className="link-quiet inline-flex items-center gap-1.5 text-paper">
                      {row.what}
                      <ArrowUpRight weight="bold" aria-hidden className="size-[0.95em]" />
                    </a>
                  </th>
                  <td className="px-5 py-4 text-plastic">{row.use}</td>
                  <td className="px-5 py-4 text-plastic">{row.author}</td>
                  <td className="px-5 py-4 text-plastic">{row.license}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </main>
      <Footer />
    </>
  );
}
