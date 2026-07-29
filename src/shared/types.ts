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

/** 把樣板拆成「會送出的前綴」與「留給使用者在終端機自行輸入的提示」。 */
export function splitTemplate(template: string): { prefix: string; hint: string } {
  const index = template.search(/\{[^}]*\}/);
  if (index < 0) {
    return { prefix: template, hint: "" };
  }
  return { prefix: template.slice(0, index), hint: template.slice(index) };
}
