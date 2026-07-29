import {
  WsMessageJson,
  RequestJson,
  type WsMessage,
  type RequestBody,
  type RequestResponseMap,
} from "@/schemas/ws";
import type { ZodError } from "zod";

export type WsStatus = "CONNECTING" | "CONNECTED" | "DISCONNECTED";

type MessageHandler = (msg: WsMessage) => void;
type StatusHandler = (status: WsStatus) => void;

type WsClientError = { type: "ZodError"; error: ZodError } | { type: "ws"; error: Event };

type ErrorHandler = (error: WsClientError) => void;

interface PendingRequest {
  // The type of `value` is inferred later in the request method
  resolve: (value: any) => void;
  reject: (reason: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

class WsClient {
  private url: string;
  private ws: WebSocket | null = null;
  private reconnectTimeout: number | null = null;
  private autoReconnect: boolean;
  private reconnectInterval: number;

  private messageListeners = new Set<MessageHandler>();
  private statusListeners = new Set<StatusHandler>();
  private errorListeners = new Set<ErrorHandler>();

  private pendingRequests = new Map<string, PendingRequest>();

  public status: WsStatus = "DISCONNECTED";

  constructor(url: string, autoReconnect = true, reconnectInterval = 2000) {
    this.url = url;
    this.autoReconnect = autoReconnect;
    this.reconnectInterval = reconnectInterval;
  }

  // Returns true if already connected
  public connect(): boolean {
    if (this.status === "CONNECTING") {
      return false;
    }

    if (this.ws?.readyState === WebSocket.OPEN) {
      return true;
    }

    this.setStatus("CONNECTING");
    this.ws = new WebSocket(this.url);

    this.ws.onopen = () => {
      this.setStatus("CONNECTED");
    };

    this.ws.onmessage = (event) => {
      this.handleRawMessage(event.data);
    };

    this.ws.onerror = (err) => {
      this.errorListeners.forEach((listener) => listener({ type: "ws", error: err }));
    };

    this.ws.onclose = () => {
      this.setStatus("DISCONNECTED");
      this.ws = null;
      this.rejectAllPending("WebSocket connection closed");

      if (this.autoReconnect) {
        this.reconnectTimeout = setTimeout(() => this.connect(), this.reconnectInterval);
      }
    };

    return false;
  }

  public disconnect() {
    if (this.reconnectTimeout) {
      clearTimeout(this.reconnectTimeout);
    }

    this.rejectAllPending("Client requested disconnect");

    if (this.ws) {
      this.ws.onclose = null;
      this.ws.close();
      this.ws = null;
    }

    this.setStatus("DISCONNECTED");
  }

  public async request<T extends RequestBody["type"]>(
    body: Extract<RequestBody, { type: T }>,
    timeoutMs = 10000,
  ): Promise<RequestResponseMap[T]> {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
      throw new Error("Cannot send request: WebSocket is not connected");
    }

    const id = crypto.randomUUID();

    const encodedResult = RequestJson.safeEncode({ id, ...body });
    if (!encodedResult.success) {
      throw new Error(`Failed to encode request: ${encodedResult.error.message}`);
    }

    return new Promise<RequestResponseMap[T]>((resolve, reject) => {
      const timer = setTimeout(() => {
        if (this.pendingRequests.has(id)) {
          this.pendingRequests.delete(id);
          reject(new Error(`Request '${id}' timed out after ${timeoutMs}ms`));
        }
      }, timeoutMs);

      this.pendingRequests.set(id, { resolve, reject, timer });
      this.ws?.send(encodedResult.data);
    });
  }

  public onMessage(listener: MessageHandler): () => void {
    this.messageListeners.add(listener);
    return () => this.messageListeners.delete(listener);
  }

  public onStatusChange(listener: StatusHandler): () => void {
    this.statusListeners.add(listener);
    listener(this.status);
    return () => this.statusListeners.delete(listener);
  }

  public onError(listener: ErrorHandler): () => void {
    this.errorListeners.add(listener);
    return () => this.errorListeners.delete(listener);
  }

  private setStatus(newStatus: WsStatus) {
    this.status = newStatus;
    this.statusListeners.forEach((listener) => listener(newStatus));
  }

  private handleRawMessage(rawData: string) {
    const result = WsMessageJson.safeDecode(rawData);

    if (!result.success) {
      this.errorListeners.forEach((listener) =>
        listener({ type: "ZodError", error: result.error }),
      );
      return;
    }

    const msg = result.data;

    if (msg.type === "response") {
      const { id, ...responseBody } = msg.data;
      const pending = this.pendingRequests.get(id);

      if (pending) {
        clearTimeout(pending.timer);
        this.pendingRequests.delete(id);

        if (responseBody.type === "error") {
          pending.reject(
            new Error(`Server Error (${responseBody.data.code}): ${responseBody.data.message}`),
          );
        } else {
          pending.resolve(responseBody);
        }

        return;
      }
    }

    // Broadcast pub/sub messages (e.g. sync_progress, current_status)
    this.messageListeners.forEach((listener) => listener(msg));
  }

  private rejectAllPending(reason: string) {
    for (const [id, pending] of this.pendingRequests.entries()) {
      clearTimeout(pending.timer);
      pending.reject(new Error(`Request '${id}' failed: ${reason}`));
    }
    this.pendingRequests.clear();
  }
}

export const defaultWsClient = new WsClient("/ws");
