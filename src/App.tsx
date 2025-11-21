import { RouterProvider, createRouter } from "@tanstack/react-router";
import { useConnectionContext } from "@/contexts/ConnectionContext";

// Import the generated route tree
import { routeTree } from "./routeTree.gen";

// Create a new router instance
const router = createRouter({
  routeTree,
  context: {
    connectionId: null,
    connectionProfile: undefined,
  },
});

// Register the router instance for type safety
declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}

function App() {
  const context = useConnectionContext();

  return <RouterProvider router={router} context={context} />;
}

export default App;
