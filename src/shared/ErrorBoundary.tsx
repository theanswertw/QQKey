import { Component, type ReactNode } from "react";

interface Props {
  children: ReactNode;
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
        <p className="crash__title">畫面出了問題</p>
        <p className="crash__message">{this.state.message}</p>
        <button
          type="button"
          className="crash__button"
          onClick={() => window.location.reload()}
        >
          重新載入
        </button>
      </div>
    );
  }
}
