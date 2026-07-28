export interface SvnProject {
  id: string;
  name: string;
  svn_url: string;
  local_path: string;
  username: string;
  password: string;
  last_rev?: number;
}

export interface CommitRecord {
  revision: number;
  author: string;
  date: string;
  message: string;
  changed_paths: string[];
}

export interface Settings {
  output_dir: string;
  excludes: string[];
  sensitive_files: string[];
}
