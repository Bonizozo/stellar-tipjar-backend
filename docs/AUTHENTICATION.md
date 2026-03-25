# Authentication Guide

The Nova Launch API currently uses JWT-based authentication for Administrative operations.

---

## Authentication Methods

### JWT (JSON Web Token)
Used for all administrative and write-access restricted endpoints. The token must be included in the HTTP `Authorization` header using the `Bearer` scheme.

```http
Authorization: Bearer <token_string>
```

#### Issuing Tokens
Tokens are typically issued through the administrative login endpoint (if enabled) or through a secure management console.

### API Keys (Planned)
Future support for API keys is planned for programmatic service-to-service communication.

---

## Public Access
The following operations are **publicly accessible** without an authentication token:
- Searching for creators (`GET /creators/search`)
- Viewing a creator profile (`GET /creators/:username`)
- Listing tips for a creator (`GET /creators/:username/tips`)
- Recording a tip (`POST /tips`)

---

## Security Best Practices
1. **Always use HTTPS**: Tokens are sent in plain text within the header and must be protected by TLS.
2. **Short-lived tokens**: Tokens should have a short expiration time (e.g., 1 hour).
3. **Secure Secrets**: Ensure that the `JWT_SECRET` used for signing is stored as a secure environment variable and never committed to version control.
4. **Credential Confidentiality**: Never share your JWT tokens; they grant full access within their defined scope.
