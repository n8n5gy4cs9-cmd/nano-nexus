import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useState } from "react";
import { RouterProvider } from "react-router-dom";

import { TooltipProvider } from "@/components/ui";
import { Toaster } from "@/components/ui/toaster";

import { IS_WEB } from "./lib/target";
import { createAppRouter } from "./routes";
import { WebGate } from "./web/WebGate";

export function App() {
  const [queryClient] = useState(() => new QueryClient({ defaultOptions: { queries: { refetchOnWindowFocus: false } } }));
  const [router] = useState(createAppRouter);
  return (
    <QueryClientProvider client={queryClient}>
      <TooltipProvider delayDuration={300}>
        {IS_WEB ? (
          <WebGate>
            <RouterProvider router={router} />
          </WebGate>
        ) : (
          <RouterProvider router={router} />
        )}
        <Toaster />
      </TooltipProvider>
    </QueryClientProvider>
  );
}
