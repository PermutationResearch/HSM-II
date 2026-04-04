"use client";

import { Dispatch, SetStateAction } from "react";

type GoalRowUi = {
  id: string;
  parent_goal_id: string | null;
  title: string;
  status: string;
};

type GovEvent = {
  id: string;
  actor: string;
  action: string;
  subject_type: string;
  subject_id: string;
  created_at: string;
};

type Props = {
  api: string;
  coSel: string;
  coErrSetter: Dispatch<SetStateAction<string | null>>;
  loadCompanyOs: () => Promise<void>;
  coNewGoalTitle: string;
  setCoNewGoalTitle: Dispatch<SetStateAction<string>>;
  coNewGoalParent: string;
  setCoNewGoalParent: Dispatch<SetStateAction<string>>;
  coGovActor: string;
  setCoGovActor: Dispatch<SetStateAction<string>>;
  coGovAction: string;
  setCoGovAction: Dispatch<SetStateAction<string>>;
  coGovSubjT: string;
  setCoGovSubjT: Dispatch<SetStateAction<string>>;
  coGovSubjId: string;
  setCoGovSubjId: Dispatch<SetStateAction<string>>;
  coGoalsSorted: GoalRowUi[];
  coGoalDepth: Map<string, number>;
  coEditGoal: string | null;
  setCoEditGoal: Dispatch<SetStateAction<string | null>>;
  coEditGoalTitle: string;
  setCoEditGoalTitle: Dispatch<SetStateAction<string>>;
  coEditGoalStatus: string;
  setCoEditGoalStatus: Dispatch<SetStateAction<string>>;
  coEditGoalParent: string;
  setCoEditGoalParent: Dispatch<SetStateAction<string>>;
  coGovernance: GovEvent[];
};

export function GoalGovernancePanel(props: Props) {
  const {
    api,
    coSel,
    coErrSetter,
    loadCompanyOs,
    coNewGoalTitle,
    setCoNewGoalTitle,
    coNewGoalParent,
    setCoNewGoalParent,
    coGovActor,
    setCoGovActor,
    coGovAction,
    setCoGovAction,
    coGovSubjT,
    setCoGovSubjT,
    coGovSubjId,
    setCoGovSubjId,
    coGoalsSorted,
    coGoalDepth,
    coEditGoal,
    setCoEditGoal,
    coEditGoalTitle,
    setCoEditGoalTitle,
    coEditGoalStatus,
    setCoEditGoalStatus,
    coEditGoalParent,
    setCoEditGoalParent,
    coGovernance,
  } = props;

  return (
    <>
      <div className="mb-4 grid gap-4 md:grid-cols-2">
        <div className="rounded border border-line bg-panel p-3">
          <div className="mb-2 text-xs uppercase text-gray-500">New goal</div>
          <input
            className="mb-2 w-full rounded border border-line bg-ink px-2 py-1 text-sm"
            placeholder="title"
            value={coNewGoalTitle}
            onChange={(e) => setCoNewGoalTitle(e.target.value)}
          />
          <input
            className="mb-2 w-full rounded border border-line bg-ink px-2 py-1 font-mono text-[11px]"
            placeholder="parent goal UUID (optional)"
            value={coNewGoalParent}
            onChange={(e) => setCoNewGoalParent(e.target.value)}
          />
          <button
            type="button"
            className="rounded bg-accent/20 px-3 py-1 text-sm text-accent"
            onClick={async () => {
              coErrSetter(null);
              try {
                const pid = coNewGoalParent.trim();
                const r = await fetch(`${api}/api/company/companies/${coSel}/goals`, {
                  method: "POST",
                  headers: { "Content-Type": "application/json" },
                  body: JSON.stringify({
                    title: coNewGoalTitle.trim(),
                    parent_goal_id: pid || undefined,
                  }),
                });
                const j = await r.json();
                if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                setCoNewGoalTitle("");
                setCoNewGoalParent("");
                await loadCompanyOs();
              } catch (e) {
                coErrSetter(e instanceof Error ? e.message : String(e));
              }
            }}
          >
            Add goal
          </button>
        </div>

        <div className="rounded border border-line bg-panel p-3">
          <div className="mb-2 text-xs uppercase text-gray-500">Log governance event</div>
          <input
            className="mb-2 w-full rounded border border-line bg-ink px-2 py-1 text-sm"
            placeholder="actor"
            value={coGovActor}
            onChange={(e) => setCoGovActor(e.target.value)}
          />
          <input
            className="mb-2 w-full rounded border border-line bg-ink px-2 py-1 text-sm"
            placeholder="action"
            value={coGovAction}
            onChange={(e) => setCoGovAction(e.target.value)}
          />
          <input
            className="mb-2 w-full rounded border border-line bg-ink px-2 py-1 text-sm"
            placeholder="subject_type (e.g. company, task)"
            value={coGovSubjT}
            onChange={(e) => setCoGovSubjT(e.target.value)}
          />
          <input
            className="mb-2 w-full rounded border border-line bg-ink px-2 py-1 font-mono text-[11px]"
            placeholder="subject_id (UUID)"
            value={coGovSubjId}
            onChange={(e) => setCoGovSubjId(e.target.value)}
          />
          <button
            type="button"
            className="rounded bg-accent/20 px-3 py-1 text-sm text-accent"
            onClick={async () => {
              coErrSetter(null);
              try {
                const r = await fetch(`${api}/api/company/companies/${coSel}/governance/events`, {
                  method: "POST",
                  headers: { "Content-Type": "application/json" },
                  body: JSON.stringify({
                    actor: coGovActor.trim(),
                    action: coGovAction.trim(),
                    subject_type: coGovSubjT.trim(),
                    subject_id: coGovSubjId.trim(),
                    payload: {},
                  }),
                });
                const j = await r.json();
                if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                await loadCompanyOs();
              } catch (e) {
                coErrSetter(e instanceof Error ? e.message : String(e));
              }
            }}
          >
            Append event
          </button>
        </div>
      </div>

      <div className="mb-4 rounded border border-line bg-panel">
        <div className="border-b border-line px-3 py-2 text-xs uppercase text-gray-500">Goals (tree)</div>
        <ul className="divide-y divide-line">
          {coGoalsSorted.map((g) => {
            const depth = coGoalDepth.get(g.id) ?? 0;
            return (
              <li key={g.id} className="px-3 py-2 text-sm" style={{ paddingLeft: 12 + depth * 14 }}>
                <div className="flex flex-wrap items-baseline justify-between gap-2">
                  <div>
                    <span className="font-medium text-gray-200">{g.title}</span>
                    <span className="ml-2 text-xs text-gray-500">{g.status}</span>
                  </div>
                  <button
                    type="button"
                    className="text-xs text-accent hover:underline"
                    onClick={() => {
                      setCoEditGoal(g.id);
                      setCoEditGoalTitle(g.title);
                      setCoEditGoalStatus(g.status);
                      setCoEditGoalParent(g.parent_goal_id ? String(g.parent_goal_id) : "");
                    }}
                  >
                    Edit
                  </button>
                </div>
                {coEditGoal === g.id && (
                  <div className="mt-2 space-y-2 rounded border border-line bg-ink/50 p-2">
                    <input
                      className="w-full rounded border border-line bg-ink px-2 py-1 text-sm"
                      value={coEditGoalTitle}
                      onChange={(e) => setCoEditGoalTitle(e.target.value)}
                    />
                    <input
                      className="w-full rounded border border-line bg-ink px-2 py-1 text-sm"
                      placeholder="status"
                      value={coEditGoalStatus}
                      onChange={(e) => setCoEditGoalStatus(e.target.value)}
                    />
                    <input
                      className="w-full rounded border border-line bg-ink px-2 py-1 font-mono text-[11px]"
                      placeholder="parent goal UUID (empty = root)"
                      value={coEditGoalParent}
                      onChange={(e) => setCoEditGoalParent(e.target.value)}
                    />
                    <div className="flex flex-wrap gap-2">
                      <button
                        type="button"
                        className="rounded bg-accent/20 px-2 py-1 text-xs text-accent"
                        onClick={async () => {
                          coErrSetter(null);
                          try {
                            const p = coEditGoalParent.trim();
                            const r = await fetch(`${api}/api/company/companies/${coSel}/goals/${g.id}`, {
                              method: "PATCH",
                              headers: { "Content-Type": "application/json" },
                              body: JSON.stringify({
                                title: coEditGoalTitle.trim(),
                                status: coEditGoalStatus.trim(),
                                parent_goal_id: p || null,
                              }),
                            });
                            const j = await r.json();
                            if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                            setCoEditGoal(null);
                            await loadCompanyOs();
                          } catch (e) {
                            coErrSetter(e instanceof Error ? e.message : String(e));
                          }
                        }}
                      >
                        Save
                      </button>
                      <button
                        type="button"
                        className="rounded px-2 py-1 text-xs text-gray-500 hover:text-gray-300"
                        onClick={() => setCoEditGoal(null)}
                      >
                        Cancel
                      </button>
                    </div>
                  </div>
                )}
              </li>
            );
          })}
          {!coGoalsSorted.length && <li className="px-3 py-4 text-gray-600">No goals.</li>}
        </ul>
      </div>

      <div className="mb-4 rounded border border-line bg-panel">
        <div className="border-b border-line px-3 py-2 text-xs uppercase text-gray-500">Governance log</div>
        <ul className="max-h-[240px] divide-y divide-line overflow-auto">
          {coGovernance.map((ev) => (
            <li key={ev.id} className="px-3 py-2 font-mono text-[11px] text-gray-400">
              <span className="text-gray-500">{ev.created_at}</span> · {ev.actor} · {ev.action} · {ev.subject_type}/{ev.subject_id}
            </li>
          ))}
          {!coGovernance.length && <li className="px-3 py-4 text-gray-600">No events.</li>}
        </ul>
      </div>
    </>
  );
}

