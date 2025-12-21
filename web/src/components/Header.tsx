import { Link } from "@tanstack/react-router";
import { Home, Menu, Monitor } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from "@/components/ui/sheet";
import { cn } from "@/lib/utils";

export default function Header() {
  return (
    <>
      <header className="p-4 flex items-center bg-background border-b border-border shadow-sm">
        <Sheet>
          <SheetTrigger asChild>
            <Button variant="ghost" size="icon" aria-label="Open menu">
              <Menu size={24} />
            </Button>
          </SheetTrigger>
          <SheetContent side="left" className="w-80">
            <SheetHeader>
              <SheetTitle>Navigation</SheetTitle>
            </SheetHeader>
            <nav className="flex flex-col gap-2 mt-6">
              <Link
                to="/"
                className={cn(
                  "flex items-center gap-3 p-3 rounded-lg hover:bg-accent transition-colors"
                )}
              >
                <Home size={20} />
                <span className="font-medium">Home</span>
              </Link>

              <Link
                to="/viewer"
                className={cn(
                  "flex items-center gap-3 p-3 rounded-lg hover:bg-accent transition-colors"
                )}
              >
                <Monitor size={20} />
                <span className="font-medium">Viewer</span>
              </Link>
            </nav>
          </SheetContent>
        </Sheet>
        <h1 className="ml-4 text-xl font-semibold text-foreground">
          <Link to="/">RemoteRG</Link>
        </h1>
      </header>
    </>
  );
}
