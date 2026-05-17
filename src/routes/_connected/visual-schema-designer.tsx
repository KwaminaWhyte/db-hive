import { createFileRoute } from "@tanstack/react-router";
import { useConnectionContext } from "@/contexts/ConnectionContext";
import VisualSchemaDesigner from "@/components/VisualSchemaDesigner";

export const Route = createFileRoute("/_connected/visual-schema-designer")({
  component: VisualSchemaDesignerRoute,
});

function VisualSchemaDesignerRoute() {
  const { connectionId } = useConnectionContext();
  return <VisualSchemaDesigner connectionId={connectionId!} />;
}
