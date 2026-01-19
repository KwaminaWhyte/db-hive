import { createFileRoute, redirect } from "@tanstack/react-router";

// Redirect /connections to the home page
// The home page now handles all connection management
export const Route = createFileRoute("/connections")({
  beforeLoad: () => {
    throw redirect({ to: "/" });
  },
  component: () => null,
});
