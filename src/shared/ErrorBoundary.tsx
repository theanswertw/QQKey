import { Component, type ReactNode } from "react";

interface Props {
  children: ReactNode;
  /**
   * 由 `main.tsx` 傳入已翻譯的字串。
   *
   * 這個元件**刻意不 import i18n**：它攔的就是 render 期間的例外，其中包含
   * i18n 自己出問題的情況，所以它必須是最後一道、不能再依賴任何會壞的東西。
   * 預設值寫死英文，i18n 初始化失敗時至少還講得出話。
   *
   * 已知取捨：這兩條字串在 bootstrap 時就固定，切換語言不會更新。可以接受——
   * 它只在崩潰後出現，而使用者接著就會按重新載入。
   */
  title?: string;
  retryLabel?: string;
}

interface State {
  message: string | null;
}

/**
 * 攔下 render 期間的例外。
 *
 * 候選框是無邊框透明視窗，React 一旦拋錯整棵樹就會卸載——留下的是一個
 * 看不見、也沒有關閉鈕的空視窗，使用者只能去工作管理員找。至少要畫出
 * 一個講得出話、按得下去的東西。
 */
export default class ErrorBoundary extends Component<Props, State> {
  state: State = { message: null };

  static getDerivedStateFromError(error: unknown): State {
    return { message: String(error) };
  }

  componentDidCatch(error: unknown) {
    console.error("畫面發生未處理的例外", error);
  }

  render() {
    if (this.state.message === null) {
      return this.props.children;
    }

    return (
      <div className="crash" role="alert">
        <p className="crash__title">{this.props.title ?? "Something went wrong"}</p>
        <p className="crash__message">{this.state.message}</p>
        <button
          type="button"
          className="crash__button"
          onClick={() => window.location.reload()}
        >
          {this.props.retryLabel ?? "Reload"}
        </button>
      </div>
    );
  }
}
