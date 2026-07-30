/** 候選項目來源。使用者自訂 > 內建目錄 > 歷史紀錄，同分時依此優先。 */
export type CandidateSource = "user" | "builtin" | "history";

/** 對應後端 `catalog::Candidate`。 */
export interface Candidate {
  id: number;
  /** 完整命令樣板，`{name}` 為待填參數，例如 `usbipd attach --wsl --busid {busid}` */
  template: string;
  /** 繁體中文說明 */
  description: string | null;
  source: CandidateSource;
  /** frecency 累計分數，供 UI 顯示常用程度 */
  score: number;
}

/** 對應後端 `catalog::EntryView`。設定畫面用，含停用中的條目。 */
export interface EntryView {
  id: number;
  template: string;
  description: string | null;
  keywords: string | null;
  source: CandidateSource;
  enabled: boolean;
  score: number;
  boost: number;
  lastUsed: number | null;
}

export interface EntryPage {
  total: number;
  entries: EntryView[];
}

/** 條目的可編輯欄位，對應後端 `catalog::EntryPatch`。 */
export interface EntryPatch {
  template: string;
  description: string | null;
  keywords: string | null;
  enabled?: boolean;
  boost?: number;
}

export interface Settings {
  shortcut: string;
  /**
   * 目前真正按得出來的快捷鍵。設定的組合被其他程式佔用時會退回預設，
   * 這時它跟 `shortcut` 不一樣；空字串代表一個都沒註冊成功。
   */
  activeShortcut: string;
  historyImport: boolean;
  secretPattern: string;
  defaultSecretPattern: string;
  launcherOpacity: number;
  defaultLauncherOpacity: number;
  poolSize: number;
}

/** 匯入前的試算。覆寫沒有 undo，按下去之前要先講清楚。 */
export interface ImportPreview {
  total: number;
  added: number;
  overwritten: number;
}

export interface ImportReport {
  scanned: number;
  imported: number;
  skippedSecret: number;
  skippedNoise: number;
}

export const SOURCE_LABELS: Record<CandidateSource, string> = {
  user: "自訂",
  builtin: "內建",
  history: "歷史",
};

/** 把樣板拆成「會送出的前綴」與「留給使用者在終端機自行輸入的提示」。 */
export function splitTemplate(template: string): { prefix: string; hint: string } {
  const index = template.search(/\{[^}]*\}/);
  if (index < 0) {
    return { prefix: template, hint: "" };
  }
  return { prefix: template.slice(0, index), hint: template.slice(index) };
}
