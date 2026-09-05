const NAV_ROWS = [64, 52, 78, 46, 70, 82, 40, 56, 88, 48];
const CARD_ROWS = [3, 2, 3];

/** The page's own shape while the CLI is read, so the content lands in place
 *  rather than replacing a centred sentence with a different layout. */
export function SettingsSkeleton() {
  return (
    <div className="set-shell set-skeleton" aria-busy="true" aria-label="Reading configuration">
      <nav className="set-nav">
        <span className="set-nav-brand">Aster</span>
        <div className="sk sk-search" />
        <ul>
          {NAV_ROWS.map((width, i) => (
            <li key={i} className="set-nav-item">
              <span className="sk sk-text" style={{ width }} />
            </li>
          ))}
        </ul>
      </nav>
      <main className="set-main">
        <header className="set-head">
          <div>
            <span className="sk sk-title" />
            <span className="sk sk-text" style={{ width: 220, marginTop: 8 }} />
          </div>
          <span className="sk sk-switch" />
        </header>
        {CARD_ROWS.map((rows, i) => (
          <section key={i}>
            {i > 0 && <span className="sk sk-text" style={{ width: 56, marginBottom: 10 }} />}
            <div className="set-card">
              {Array.from({ length: rows }, (_, j) => (
                <div key={j} className="set-row">
                  <div className="set-row-text">
                    <span className="sk sk-text" style={{ width: 96 + ((i * 3 + j) % 4) * 28 }} />
                    <span
                      className="sk sk-text sk-dim"
                      style={{ width: 200 + ((i + j) % 3) * 60, marginTop: 8 }}
                    />
                  </div>
                  <span className="sk sk-control" />
                </div>
              ))}
            </div>
          </section>
        ))}
      </main>
    </div>
  );
}
