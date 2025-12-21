import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Label } from "@/components/ui/label";
import { ThemeToggle } from "@/components/theme-toggle";

export const Route = createFileRoute("/")({ component: App });

function App() {
  const [sessionId, setSessionId] = useState<string>("fixed");
  const [codec, setCodec] = useState<"h264" | "any">("h264");
  const navigate = useNavigate();

  const handleConnect = () => {
    navigate({
      to: "/viewer/$sessionId/$codec",
      params: {
        sessionId: sessionId || "fixed",
        codec: codec || "h264",
      },
    });
  };

  return (
    <div className="min-h-screen bg-background flex items-center justify-center relative">
      <div className="absolute top-4 right-4">
        <ThemeToggle />
      </div>
      <div className="w-full max-w-md space-y-6">
        <div className="text-center">
          <h1 className="text-4xl md:text-6xl font-bold text-foreground mb-8">
            RemoteRG
          </h1>
        </div>

        <Card>
          <CardHeader>
            <CardTitle>接続設定</CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="session-id">セッションID:</Label>
              <Input
                id="session-id"
                type="text"
                value={sessionId}
                onChange={(e) => setSessionId(e.target.value)}
                placeholder="fixed"
                className="w-full"
              />
            </div>

            <div className="space-y-2">
              <Label>コーデック:</Label>
              <RadioGroup
                value={codec}
                onValueChange={(value) => setCodec(value as "h264" | "any")}
                className="flex gap-4"
              >
                <div className="flex items-center space-x-2">
                  <RadioGroupItem value="h264" id="h264" />
                  <Label htmlFor="h264" className="cursor-pointer">
                    H.264
                  </Label>
                </div>
                <div className="flex items-center space-x-2">
                  <RadioGroupItem value="any" id="any" />
                  <Label htmlFor="any" className="cursor-pointer">
                    自動
                  </Label>
                </div>
              </RadioGroup>
            </div>

            <Button size="lg" className="w-full" onClick={handleConnect}>
              接続
            </Button>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
