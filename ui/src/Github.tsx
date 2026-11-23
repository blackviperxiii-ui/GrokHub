import { useEffect, useState } from "react";
import { getGithub, runGithub, saveGithubToken, type GithubRan, type GithubTool, type GithubView } from "./api";
import { ConfirmRow, StatusLine, useNote } from "./ui";

export const GITHUB_NOTE = "Read-only. Who am I and List repos use your token. Nothing is ever written to GitHub.";

export const TOKEN_SET = "A token is saved in your keyring.";

export const TOKEN_UNSET =
  "Paste a personal access token that can read your repositories. It's saved in your keyring, never in a file.";

const TOOLS: { tool: GithubTool; label: string }[] = [
  { tool: "user", label: "Who am I" },
  { tool: "listRepos", label: "List repos" },
];

/** Connectors → GitHub: a personal access token and two read-only calls. */
export function Github() {
  const [view, setView] = useState<GithubView | null>(null);
  const [token, setToken] = useState("");
  const [ran, setRan] = useState<GithubRan | null>(null);
  const [busy, setBusy] = useState(false);
  const [removing, setRemoving] = useState(false);
  const { note, ok, fail } = useNote();

  useEffect(() => {
    getGithub().then(setView, fail);
  }, [fail]);

  async function save(next: string) {
    setBusy(true);
    setRemoving(false);
    try {
      const saved = await saveGithubToken(next);
      setView(saved);
      setRan(null);
      ok(saved.status);
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  }

  async function run(tool: GithubTool) {
    setBusy(true);
    try {
      const answer = await runGithub(tool);
      setRan(answer);
      ok(answer.summary);
    } catch (err) {
      setRan(null);
      fail(err);
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="github" aria-label="GitHub">
      <h2>GitHub</h2>
      <p className="note">{GITHUB_NOTE}</p>
      {view && <p className="note">{view.tokenSet ? TOKEN_SET : TOKEN_UNSET}</p>}
      <form
        className="card-buttons"
        onSubmit={(e) => {
          e.preventDefault();
          const sent = token;
          // The token leaves the page's state as soon as it is sent.
          setToken("");
          void save(sent);
        }}
      >
        <input
          type="password"
          autoComplete="off"
          spellCheck={false}
          aria-label="GitHub token"
          placeholder="Personal access token"
          value={token}
          onChange={(e) => setToken(e.target.value)}
        />
        <button type="submit" disabled={busy || !token.trim()}>
          Save token
        </button>
        {view?.tokenSet && !removing && (
          <button type="button" className="quiet" disabled={busy} onClick={() => setRemoving(true)}>
            Remove token…
          </button>
        )}
      </form>
      {removing && (
        <ConfirmRow
          question="Remove the GitHub token?"
          detail="Who am I and List repos stop working until you paste a token again."
          action="Remove"
          busy={busy}
          onConfirm={() => void save("")}
          onCancel={() => setRemoving(false)}
        />
      )}
      <div className="card-buttons">
        {TOOLS.map(({ tool, label }) => (
          <button
            key={tool}
            type="button"
            className="quiet"
            disabled={busy || !view?.tokenSet}
            onClick={() => void run(tool)}
          >
            {label}
          </button>
        ))}
      </div>
      <StatusLine note={note} />
      {ran && ran.repos.length > 0 && (
        <ul className="rows" aria-label="GitHub repositories">
          {ran.repos.map((repo) => (
            <li key={repo.name} className="card github-repo" aria-label={repo.name}>
              <div className="card-title">{repo.name}</div>
              <div className="card-status">{repo.private ? "Private" : "Public"}</div>
              {repo.description && <div>{repo.description}</div>}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
