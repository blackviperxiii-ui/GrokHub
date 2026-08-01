export type HostFileEntry = {
  name: string;
  path: string;
  isDir: boolean;
  size: number;
  mtimeMs: number;
};

export type HostExecResult = {
  ok: boolean;
  code: number | null;
  stdout: string;
  stderr: string;
  cwd: string;
  command: string;
  ms: number;
};

export type HostApp = {
  id: string;
  name: string;
  exec: string;
  desktopFile: string;
  terminal: boolean;
};

export type HostInfo = {
  platform: string;
  arch: string;
  homedir: string;
  cwd: string;
  user: string;
  shell: string;
  hostname: string;
  bridge: "electron" | "server" | "none";
  unsandboxed: boolean;
};
