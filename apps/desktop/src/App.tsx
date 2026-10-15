import { Logo } from "./components/Logo";
import { Mono } from "./components/ui";
import { displayPath } from "./lib/format";
import { Diagnostics } from "./screens/Diagnostics";
import { History } from "./screens/History";
import { Overview } from "./screens/Overview";
import { Profiles } from "./screens/Profiles";
import { Repository } from "./screens/Repository";
import { Rules } from "./screens/Rules";
import { Settings } from "./screens/Settings";
import { AppProvider, useApp, type Screen } from "./state";

const primary: { id: Screen; label: string }[] = [
  { id: "overview", label: "Overview" },
  { id: "repository", label: "Repository" },
  { id: "diagnostics", label: "Diagnostics" },
  { id: "profiles", label: "Profiles" },
  { id: "rules", label: "Rules" },
  { id: "history", label: "History" },
];

function NavButton({ id, label }: { id: Screen; label: string }) {
  const { screen, go } = useApp();
  const active = screen === id;
  return (
    <button
      type="button"
      aria-current={active ? "page" : undefined}
      onClick={() => go(id)}
      className={`flex h-8 w-full items-center rounded-md px-3 text-left text-base ${
        active ? "bg-accent-soft font-medium text-accent-strong" : "text-ink hover:bg-line/60"
      }`}
    >
      {label}
    </button>
  );
}

function Sidebar() {
  const { repository, home, info, go } = useApp();
  const name = repository?.split("/").filter(Boolean).pop();
  return (
    <aside className="flex flex-col gap-4 border-b border-line bg-sidebar px-3 py-4 md:w-52 md:shrink-0 md:border-r md:border-b-0">
      <div className="flex items-center gap-2 px-2">
        <Logo className="size-6" />
        <span className="text-[0.95rem] font-semibold tracking-tight">GitMate</span>
      </div>
      <nav aria-label="Main" className="flex flex-row flex-wrap gap-0.5 md:flex-1 md:flex-col md:flex-nowrap">
        {primary.map((item) => (
          <NavButton key={item.id} {...item} />
        ))}
        <div className="md:mt-auto">
          <NavButton id="settings" label="Settings" />
        </div>
      </nav>
      {repository && (
        <button
          type="button"
          onClick={() => go("repository")}
          className="hidden flex-col items-start gap-0.5 rounded-md border border-line bg-canvas px-3 py-2 text-left hover:border-line-strong md:flex"
        >
          <span className="text-xs text-muted-text">Current repository</span>
          <span className="w-full truncate font-medium">{name}</span>
          <Mono className="w-full truncate text-muted-text">{displayPath(repository, home)}</Mono>
        </button>
      )}
      {info && (
        <p className="hidden px-2 text-xs text-muted-text md:block">
          {info.git_version ? `Git ${info.git_version.text}` : "Git not found"}
        </p>
      )}
    </aside>
  );
}

function CurrentScreen() {
  const { screen } = useApp();
  switch (screen) {
    case "overview":
      return <Overview />;
    case "repository":
      return <Repository />;
    case "diagnostics":
      return <Diagnostics />;
    case "profiles":
      return <Profiles />;
    case "rules":
      return <Rules />;
    case "history":
      return <History />;
    case "settings":
      return <Settings />;
  }
}

export default function App() {
  return (
    <AppProvider>
      <div className="flex h-full flex-col md:flex-row">
        <Sidebar />
        <main className="flex min-h-0 min-w-0 flex-1 flex-col overflow-y-auto">
          <CurrentScreen />
        </main>
      </div>
    </AppProvider>
  );
}
