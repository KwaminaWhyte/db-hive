import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { Button } from "@/components/ui/button";
import { ArrowLeft } from "lucide-react";
import { AboutContent } from "@/components/AboutContent";

export const Route = createFileRoute("/about")({
  component: AboutRoute,
});

function AboutRoute() {
  const navigate = useNavigate({ from: "/about" });

  const handleBack = () => {
    if (window.history.length > 1) {
      window.history.back();
    } else {
      navigate({ to: "/" });
    }
  };

  return (
    <div className="flex-1 h-full relative bg-background">
      {/* Back Button */}
      <div className="absolute top-4 left-4 z-50">
        <Button variant="outline" size="sm" onClick={handleBack}>
          <ArrowLeft className="size-4 mr-2" />
          Back
        </Button>
      </div>

      {/* Main Content */}
      <div className="h-full overflow-y-auto">
        <div className="max-w-4xl mx-auto px-8 py-12">
          <AboutContent variant="page" />
        </div>
      </div>
    </div>
  );
}
