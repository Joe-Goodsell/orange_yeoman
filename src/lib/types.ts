export interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
}

export interface ChangeEvent {
  path: string;
  kind: "structure" | "content";
  ts: number;
}