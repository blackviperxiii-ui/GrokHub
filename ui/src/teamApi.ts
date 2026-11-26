import { call as invoke } from "./transport";

/** The four kinds of agent the person can make. */
export type AgentKind = "role" | "family" | "staff" | "customer";
export type AgentAccess = "read_only" | "ask" | "auto";

/** One agent, as `agent-team.json` keeps it. Fields the page doesn't edit
 * (`tools`, `playbooks`) ride along unchanged. */
export interface AgentProfile {
  id: string;
  name: string;
  kind: AgentKind;
  access: AgentAccess;
  about: string;
  team?: string;
  desktop: boolean;
  tools?: string[];
  playbooks?: string[] | null;
  model_tier?: string;
  talks_to?: string[];
  /** Paired computers may message it, the ones in `share_with`. */
  share: boolean;
  /** Device ids of the paired computers that may message it. */
  share_with?: string[];
}

/** A computer paired with this one, from Devices > Share. */
export interface PairedComputer {
  /** Its device id; two computers can share a name. */
  id: string;
  name: string;
  /** Unix ms; 0 before its first call. */
  lastSeen: number;
}

export interface AgentTeam {
  id: string;
  name: string;
  /** The agent that leads it. */
  lead: string;
}

export interface TeamPage {
  agents: AgentProfile[];
  teams: AgentTeam[];
  /** The first rule the saved file breaks, after a hand edit. */
  problem: string | null;
  tiers: string[];
}

export type AgentTaskState =
  | "TASK_STATE_SUBMITTED"
  | "TASK_STATE_WORKING"
  | "TASK_STATE_INPUT_REQUIRED"
  | "TASK_STATE_AUTH_REQUIRED"
  | "TASK_STATE_COMPLETED"
  | "TASK_STATE_FAILED"
  | "TASK_STATE_CANCELED"
  | "TASK_STATE_REJECTED";

/** One message to an agent and what came back. */
export interface AgentTask {
  id: string;
  from: "person" | { agent: string } | { peer: string };
  to: string;
  state: AgentTaskState;
  asked: string;
  reply: string;
  started_ms: number;
  ended_ms: number;
}

export function getTeam(): Promise<TeamPage> {
  return invoke<TeamPage>("get_team");
}

export function saveAgent(agent: AgentProfile, add: boolean): Promise<TeamPage> {
  return invoke<TeamPage>("team_save_agent", { agent, add });
}

/** The computers paired with this one. Desktop only: elsewhere it rejects. */
export function pairedComputers(): Promise<PairedComputer[]> {
  return invoke<PairedComputer[]>("team_computers");
}

export function removeAgent(id: string): Promise<TeamPage> {
  return invoke<TeamPage>("team_remove_agent", { id });
}

export function saveTeam(team: AgentTeam, add: boolean): Promise<TeamPage> {
  return invoke<TeamPage>("team_save_team", { team, add });
}

export function removeTeam(id: string): Promise<TeamPage> {
  return invoke<TeamPage>("team_remove_team", { id });
}

/** The newest 200 messages, newest first. */
export function getAgentTasks(): Promise<AgentTask[]> {
  return invoke<AgentTask[]>("get_agent_tasks");
}

export function clearAgentTasks(): Promise<void> {
  return invoke<void>("clear_agent_tasks");
}
