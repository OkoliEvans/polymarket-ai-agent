"use client";

import { useEffect, useRef } from "react";

type SSEEvent = {
  type: "bets" | "jobs" | "ping";
  [key: string]: unknown;
};

export function useSSE(onEvent: (event: SSEEvent) => void) {
  const onEventRef = useRef(onEvent);
  onEventRef.current = onEvent;

  useEffect(() => {
    const url = `${process.env.NEXT_PUBLIC_DB_API_URL ?? "http://localhost:3001"}/events`;
    let es: EventSource;
    let reconnectTimer: ReturnType<typeof setTimeout>;

    function connect() {
      es = new EventSource(url);

      es.onmessage = (e) => {
        try {
          const data = JSON.parse(e.data) as SSEEvent;
          onEventRef.current(data);
        } catch {
          // ping or malformed — ignore
        }
      };

      es.onerror = () => {
        es.close();
        // Auto-reconnect after 3s
        reconnectTimer = setTimeout(connect, 3000);
      };
    }

    connect();

    return () => {
      clearTimeout(reconnectTimer);
      es?.close();
    };
  }, []); // stable — onEventRef handles updates
}