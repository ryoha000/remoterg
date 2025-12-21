import { createFileRoute, Link } from "@tanstack/react-router";
import { Button } from "@/components/ui/button";
import { ThemeToggle } from "@/components/theme-toggle";

export const Route = createFileRoute("/")({ component: App });

function App() {
  return (
    <div className="min-h-screen bg-background flex items-center justify-center relative">
      <div className="absolute top-4 right-4">
        <ThemeToggle />
      </div>
      <div className="text-center">
        <h1 className="text-4xl md:text-6xl font-bold text-foreground mb-8">
          RemoteRG
        </h1>
        <Button asChild size="lg">
          <Link to="/viewer">Viewerへ</Link>
        </Button>
      </div>
    </div>
  );
}
