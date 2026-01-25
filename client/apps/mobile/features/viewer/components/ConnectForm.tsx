import { View, TextInput, Button, StyleSheet, Text } from "react-native";

interface ConnectFormProps {
  sessionId: string;
  setSessionId: (id: string) => void;
  status: string;
  connect: () => void;
}

export const ConnectForm = ({ sessionId, setSessionId, status, connect }: ConnectFormProps) => {
  return (
    <View style={styles.formContainer}>
      <Text style={styles.title}>RemoteRG Mobile</Text>
      <Text style={styles.status}>{status}</Text>
      <Text style={styles.label}>Session ID</Text>
      <TextInput
        style={styles.input}
        value={sessionId}
        onChangeText={setSessionId}
        placeholder="Enter Session ID"
        autoCapitalize="none"
      />
      <Button title="Connect" onPress={connect} />
    </View>
  );
};

const styles = StyleSheet.create({
  formContainer: {
    flex: 1,
    justifyContent: 'center',
    padding: 20,
    backgroundColor: '#fff',
  },
  title: {
    fontSize: 24,
    fontWeight: 'bold',
    marginBottom: 20,
    textAlign: 'center',
  },
  status: {
    marginBottom: 20,
    textAlign: 'center',
    color: 'gray',
  },
  label: {
    marginBottom: 5,
    fontWeight: 'bold',
  },
  input: {
    borderWidth: 1,
    borderColor: '#ccc',
    padding: 10,
    marginBottom: 20,
    borderRadius: 5,
  },
});
