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

// Safe view of the merged config as produced by the Rust core. Contains no API
// key values, only provider names.
export interface ConfigStatus {
  global_loaded: boolean;
  project_loaded: boolean;
  project_path: string | null;
  small_model: string;
  large_model: string;
  configured_providers: string[];
  error: string | null;
}
