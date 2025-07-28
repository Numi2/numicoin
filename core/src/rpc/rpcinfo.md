src/rpc/
├── mod.rs          # Main RPC server and public interface
├── types.rs        # API request/response types and configurations  
├── auth.rs         # JWT authentication and authorization
├── rate_limit.rs   # Rate limiting and IP blocking logic
├── middleware.rs   # Warp filters and middleware
├── handlers.rs     # Route handlers for each endpoint
└── error.rs        # Error handling and rejection recovery

types.rs (254 lines): All API types, configurations, and utility functions
auth.rs (75 lines): JWT token management and authentication filters
rate_limit.rs (97 lines): Rate limiting logic with progressive blocking
middleware.rs (35 lines): Warp filter helpers and middleware
handlers.rs (300+ lines): Clean, focused route handlers
error.rs (32 lines): Centralized error handling
mod.rs (290 lines): Core RPC server with route configuration
