import { createFileRoute } from "@tanstack/react-router";
import { AppShell } from "@/components/AppShell";

export const Route = createFileRoute("/")({
  component: HomePage,
});

function HomePage() {
  return <AppShell />;
}
