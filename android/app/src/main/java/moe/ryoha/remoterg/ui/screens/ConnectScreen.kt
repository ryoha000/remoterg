package moe.ryoha.remoterg.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ConnectScreen(
    onConnect: (String) -> Unit
) {
    var signalingUrl by remember { mutableStateOf("ws://10.0.2.2:8787/api/signal?session_id=fixed&role=viewer") }

    Scaffold(
        topBar = {
            TopAppBar(title = { Text("RemoteRG Connect") })
        }
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(16.dp),
            verticalArrangement = Arrangement.Center,
            horizontalAlignment = Alignment.CenterHorizontally
        ) {
            OutlinedTextField(
                value = signalingUrl,
                onValueChange = { signalingUrl = it },
                label = { Text("Signaling Server URL") },
                modifier = Modifier.fillMaxWidth()
            )

            Button(
                onClick = { onConnect(signalingUrl) },
                modifier = Modifier.padding(top = 16.dp)
            ) {
                Text("Connect")
            }
        }
    }
}
