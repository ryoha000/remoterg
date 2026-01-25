---
trigger: glob
globs: client/apps/mobile/**
---

# shadcn instructions

## Add components
You can now start adding components to your app.

```
pnpm dlx @react-native-reusables/cli@latest add button
```

The command above will add the Button component to your project. You can then import it like this:

```index.tsx
import { Button } from '@/components/ui/button';
import { Text } from '@/components/ui/text';
 
export default function Screen() {
  return (
    <Button>
      <Text>Click me</Text>
    </Button>
  );
}
```