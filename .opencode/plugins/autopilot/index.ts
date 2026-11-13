/**
 * Blastit harness plugin: composition root.
 *
 *  - Autopilot: keeps the top-level conductor running while plan/AUTOPILOT is "on".
 *  - ExpensiveBudget: tracks expensive-tier spend (Gemini 4 Carbon / Opus), writes plan/BUDGET.md,
 *    and blocks expensive subagents past the cap configured in plan/BUDGET.json.
 */
import { Plugin } from "@opencode/plugin";
import { Autopilot } from "./autopilot.ts";
import { EXPENSIVE_AGENTS, ExpensiveBudget } from "./budget.ts";

const WATCHED = [
  "session.idle",
  "session.execution.succeeded",
  "session.execution.failed",
  "session.execution.interrupted",
] as const;

export default Plugin.define({
  id: "blastit.autopilot",
  async setup(ctx) {
    const root = String(ctx.location.project?.directory ?? ctx.location.directory);
    const autopilot = new Autopilot(ctx as never, root);
    const budget = new ExpensiveBudget(ctx as never, root, ctx.location.project?.id as string | undefined);
    const controller = new AbortController();

    void budget
      .backfill()
      .then(() => budget.refreshReport(true))
      .catch((error: unknown) => console.error("[blastit-budget] backfill failed", error));

    const permission = await ctx.permission.hook("evaluate", async (event) => {
      if (event.action !== "subagent") return;
      const target = event.resources.find((r) => EXPENSIVE_AGENTS.has(r));
      if (!target) return;
      try {
        const message = await budget.denyMessage(target);
        if (message) {
          event.effect = "deny";
          event.message = message;
        }
      } catch (error) {
        console.error("[blastit-budget] evaluate failed", error);
      }
    });

    void (async () => {
      try {
        for await (const event of ctx.event.subscribe({ types: [...WATCHED], signal: controller.signal })) {
          const sessionID = (event as { data?: { sessionID?: string } }).data?.sessionID;
          if (!sessionID) continue;
          try {
            await budget.recordSession(sessionID);
            await budget.refreshReport();
            if (event.type === "session.idle") await autopilot.onIdle(sessionID);
          } catch (error) {
            console.error("[blastit-autopilot] event handler failed", error);
          }
        }
      } catch (error) {
        if (!controller.signal.aborted) console.error("[blastit-autopilot] event stream failed", error);
      }
    })();

    return async () => {
      controller.abort();
      autopilot.dispose();
      await permission.dispose();
    };
  },
});
