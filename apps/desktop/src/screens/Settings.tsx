import { Button, ErrorNotice, Mono, ScreenHeader, Section } from "../components/ui";
import { displayPath } from "../lib/format";
import { useApp } from "../state";

export function Settings() {
  const { info, infoError, home, recents, removeRecent, clearRecents } = useApp();

  return (
    <>
      <ScreenHeader title="Settings" />
      <div className="flex max-w-[760px] flex-col gap-8 px-8 py-6">
        {infoError && <ErrorNotice message={infoError} />}
        <Section
          title="Recent repositories"
          description="Kept on this computer only, to make switching quicker."
          actions={
            recents.length > 0 ? (
              <Button size="sm" onClick={clearRecents}>
                Clear all
              </Button>
            ) : undefined
          }
        >
          {recents.length === 0 ? (
            <p className="text-sm text-muted-text">None yet.</p>
          ) : (
            <ul className="flex flex-col divide-y divide-line border-y border-line">
              {recents.map((r) => (
                <li key={r} className="flex items-center justify-between gap-3 py-2">
                  <Mono>{displayPath(r, home)}</Mono>
                  <Button size="sm" variant="ghost" onClick={() => removeRecent(r)} aria-label={`Forget ${r}`}>
                    Forget
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </Section>

        {info && (
          <Section title="Where GitMate keeps things">
            <dl className="grid grid-cols-[11rem_minmax(0,1fr)] gap-x-3 gap-y-2">
              <dt className="text-sm text-muted-text">Profiles</dt>
              <dd>
                <Mono>{displayPath(`${info.paths.config_dir}/profiles.toml`, home)}</Mono>
              </dd>
              <dt className="text-sm text-muted-text">Profile include files</dt>
              <dd>
                <Mono>{displayPath(`${info.paths.config_dir}/includes/`, home)}</Mono>
              </dd>
              <dt className="text-sm text-muted-text">History and backups</dt>
              <dd>
                <Mono>{displayPath(info.paths.state_dir, home)}</Mono>
              </dd>
              <dt className="text-sm text-muted-text">Rules are written to</dt>
              <dd>
                <Mono>{displayPath(info.global_config, home)}</Mono>
              </dd>
            </dl>
          </Section>
        )}

        {info && (
          <Section title="About">
            <dl className="grid grid-cols-[11rem_minmax(0,1fr)] gap-x-3 gap-y-2">
              <dt className="text-sm text-muted-text">GitMate</dt>
              <dd>{info.version}</dd>
              <dt className="text-sm text-muted-text">Git</dt>
              <dd>{info.git_version ? info.git_version.text : <span className="text-danger">{info.git_error}</span>}</dd>
            </dl>
            <p className="max-w-[68ch] text-sm text-muted-text">
              GitMate works entirely on this computer. It never stores passwords, access tokens or private key
              contents, and it does not edit <Mono>~/.ssh/config</Mono>.
            </p>
          </Section>
        )}
      </div>
    </>
  );
}
