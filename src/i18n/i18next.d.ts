import type { resources } from "./resources";

declare module "i18next" {
  interface CustomTypeOptions {
    // 單一 namespace，分組靠 key 前綴（common. / launcher. / settings.）。
    // 拆成三個 namespace 會讓兩個 bundle 各自 tree-shake，但候選框視窗是啟動時
    // 就建立並常駐隱藏的（show() 不重新解析 bundle），所以 bundle 大小只影響
    // 啟動，不影響按下快捷鍵到畫面出現的延遲。
    defaultNS: "translation";
    resources: (typeof resources)["zh-Hant"];
  }
}
