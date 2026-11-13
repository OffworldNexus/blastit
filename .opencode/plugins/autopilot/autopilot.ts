/**
 * Keep-alive for the top-level `conductor` session (see index.ts for wiring).
 *
 * Re-prompts the conductor when it goes idle while plan/AUTOPILOT is "on".
 * Never after a user interrupt (Esc); turns itself off after MAX_IDLE_WITHOUT_PROGRESS
 * idle nudges without any change to plan/JOURNAL.md; backs off after failed executions.
 */
import { appendFile, readFile, stat, writeFile } from "node:fs/promises";
import { join } from "node:path";

const AGENT = "conductor";
const MAX_IDLE_WITHOUT_PROGRESS = 3;
const NUDGE_DELAY_MS = 5_000;
const FAILURE_BACKOFF_MS = 60_000;

interface Tracker {
  idleWithoutProgress: number;
  lastJournalSize: number;
  failures: number;
}

export interface AutopilotCtx {
  session: {
    get(i: { sessionID: never }): Promise<unknown>;
    prompt(i: { sessionID: never; text: string }): Promise<unknown>;
  };
}

function stamp(): string {
  const d = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

export class Autopilot {
  private readonly trackers = new Map<string, Tracker>();
  private readonly timers = new Set<ReturnType<typeof setTimeout>>();

  constructor(
    private readonly ctx: AutopilotCtx,
    private readonly root: string,
  ) {}

  private get planDir(): string {
    return join(this.root, "plan");
  }

  private async flag(): Promise<string> {
    try {
      return (await readFile(join(this.planDir, "AUTOPILOT"), "utf8")).trim();
    } catch {
      return "off";
    }
  }

  private async journalSize(): Promise<number> {
    try {
      return (await stat(join(this.planDir, "JOURNAL.md"))).size;
    } catch {
      return 0;
    }
  }

  async onIdle(sessionID: string): Promise<void> {
    const info = (await this.ctx.session.get({ sessionID: sessionID as never })) as {
      parentID?: string;
      agent?: string;
      outcome?: "succeeded" | "failed" | "interrupted";
      location?: { directory?: string };
    };
    if (info.parentID || info.agent !== AGENT || info.outcome === "interrupted") return;
    const dir = info.location?.directory;
    if (dir && !String(dir).startsWith(this.root)) return;
    if ((await this.flag()) !== "on") return;

    const t = this.trackers.get(sessionID) ?? { idleWithoutProgress: 0, lastJournalSize: -1, failures: 0 };
    const size = await this.journalSize();
    t.idleWithoutProgress = size === t.lastJournalSize ? t.idleWithoutProgress + 1 : 0;
    t.lastJournalSize = size;
    t.failures = info.outcome === "failed" ? t.failures + 1 : 0;
    this.trackers.set(sessionID, t);

    if (t.idleWithoutProgress >= MAX_IDLE_WITHOUT_PROGRESS) {
      await writeFile(join(this.planDir, "AUTOPILOT"), "off\n", "utf8");
      await appendFile(
        join(this.planDir, "JOURNAL.md"),
        `- ${stamp()} AUTOPILOT off by plugin: ${MAX_IDLE_WITHOUT_PROGRESS} idle nudges without plan progress\n`,
        "utf8",
      );
      return;
    }

    const delay = t.failures > 0 ? FAILURE_BACKOFF_MS * Math.min(t.failures, 10) : NUDGE_DELAY_MS;
    const text = [
      `AUTOPILOT nudge (idle-without-progress ${t.idleWithoutProgress}/${MAX_IDLE_WITHOUT_PROGRESS}` +
        (t.failures ? `, after ${t.failures} failed execution(s)` : "") +
        ").",
      "plan/AUTOPILOT is on, so do not stop. Reload skills dx-first, plan-system, dispatch and git-workflow if they are not in context,",
      "then resume the conductor loop at Boot step 2 (release branch hygiene, then `uv run --no-project tools/plan.py validate` and `state`).",
      'If every actionable item is truly blocked on the human, run `uv run --no-project tools/plan.py --agent conductor autopilot off --reason "blocked on human"` and ask once.',
    ].join("\n");
    const timer = setTimeout(() => {
      this.timers.delete(timer);
      void this.ctx.session
        .prompt({ sessionID: sessionID as never, text })
        .catch((error: unknown) => console.error("[blastit-autopilot] prompt failed", error));
    }, delay);
    this.timers.add(timer);
  }

  dispose(): void {
    for (const t of this.timers) clearTimeout(t);
    this.timers.clear();
  }
}
