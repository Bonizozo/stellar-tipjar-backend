# Integration Examples

Sample code for common tasks using the Nova Launch TipJar API.

---

## Create Creator (JavaScript / Node.js)
Register a new creator on the platform.

```javascript
const axios = require('axios');

async function createProfile(username, walletAddress) {
  try {
    const response = await axios.post('http://localhost:8000/creators', {
      username: username,
      wallet_address: walletAddress
    });
    console.log('Profile created:', response.data);
  } catch (error) {
    if (error.response.status === 409) {
      console.error('Username already taken');
    } else {
      console.error('API Error:', error.response.data.error);
    }
  }
}

createProfile('alice', 'GABC...');
```

---

## Searching for Creators (Python)
Find creators with a fuzzy search term.

```python
import requests

def search_creators(query):
    base_url = "http://localhost:8000/creators/search"
    params = {"q": query, "limit": 10}
    
    response = requests.get(base_url, params=params)
    
    if response.status_code == 200:
        for creator in response.json():
            print(f"Found: {creator['username']} ({creator['wallet_address']})")
    else:
        print(f"Error: {response.json().get('error')}")

search_creators('ali')
```

---

## Record a Tip (Rust)
Record a Stellar transaction as a tip.

```rust
use reqwest::Client;
use serde::Serialize;

#[derive(Serialize)]
struct RecordTipRequest {
    username: String,
    amount: String,
    transaction_hash: String,
}

async fn record_tip() -> Result<(), reqwest::Error> {
    let client = Client::new();
    let res = client.post("http://localhost:8000/tips")
        .json(&RecordTipRequest {
            username: "alice".to_string(),
            amount: "10.0".to_string(),
            transaction_hash: "stellar_tx_hash_...".to_string(),
        })
        .send()
        .await?;

    if res.status().is_success() {
        println!("Tip recorded successfully!");
    } else {
        println!("Error: {:?}", res.status());
    }
    Ok(())
}
```

---

## Verifying Tip Webhook (Node.js/Express)
Verify the authenticity of a tip notification.

```javascript
const express = require('express');
const crypto = require('crypto');
const app = express();

app.use(express.json());

const WEBHOOK_SECRET = 'your_webhook_secret_here';

app.post('/webhook', (req, res) => {
    const signature = req.headers['x-webhook-signature'];
    const hmac = crypto.createHmac('sha256', WEBHOOK_SECRET);
    const computedSignature = hmac.update(JSON.stringify(req.body)).digest('hex');

    if (signature === computedSignature) {
        console.log('Received valid tip event:', req.body.event_type);
        // Process the tip...
        res.sendStatus(200);
    } else {
        console.warn('Invalid webhook signature!');
        res.sendStatus(401);
    }
});

app.listen(3000, () => console.log('Webhook server running...'));
```
