import { View } from "react-native";
import { Button } from "@/components/ui/button";
import { Text } from "@/components/ui/text";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";

interface ConnectFormProps {
  sessionId: string;
  setSessionId: (id: string) => void;
  status: string;
  connect: () => void;
}

export const ConnectForm = ({ sessionId, setSessionId, status, connect }: ConnectFormProps) => {
  return (
    <View className="flex-1 justify-center p-6 bg-background">
      <Card className="w-full max-w-xl mx-auto">
        <CardHeader>
          <CardTitle className="text-2xl font-bold text-center">RemoteRG Mobile</CardTitle>
          <CardDescription className="text-center">
            Enter your Session ID to connect to the host.
          </CardDescription>
        </CardHeader>
        <CardContent className="gap-4">
          <View className="gap-2">
            <Label nativeID="sessionId">Session ID</Label>
            <Input
              placeholder="Enter Session ID"
              value={sessionId}
              onChangeText={setSessionId}
              autoCapitalize="none"
              aria-labelledby="sessionId"
            />
          </View>
          {!!status && (
             <Text className="text-xs text-center text-muted-foreground">{status}</Text>
          )}
        </CardContent>
        <CardFooter>
          <Button onPress={connect} className="w-full">
            <Text>Connect</Text>
          </Button>
        </CardFooter>
      </Card>
    </View>
  );
};
