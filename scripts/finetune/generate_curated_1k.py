#!/usr/bin/env python3
"""Generate 1,000 curated training pairs for CodeLens embedding model."""
import json, hashlib, random
from pathlib import Path

random.seed(42)
ROOT = Path(__file__).parent.parent.parent
BENCHMARK = ROOT / "benchmarks" / "embedding-quality-dataset.json"
OUTPUT = Path(__file__).parent / "curated_1k_pairs.jsonl"

# Load benchmark queries to avoid
with open(BENCHMARK) as f:
    bench = json.load(f)
bench_queries = set(d["query"].lower().strip() for d in bench)

def split_id(name):
    import re
    if "_" in name:
        parts = [p for p in name.split("_") if p]
        exp = []
        for p in parts:
            s = re.sub(r"([a-z0-9])([A-Z])", r"\1 \2", p)
            s = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1 \2", s)
            exp.extend(s.split())
        return " ".join(w.lower() for w in exp if w)
    s = re.sub(r"([a-z0-9])([A-Z])", r"\1 \2", name)
    s = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1 \2", s)
    return " ".join(w.lower() for w in s.split() if w)

def pos(name, kind, file, sig=""):
    sp = split_id(name)
    ns = f"{name} ({sp})" if sp != name.lower() else name
    base = f"{kind} {ns} in {file}"
    if sig:
        base += f": {sig}"
    return base

# ========== TypeScript/JS/React: 300 pairs ==========
TS = [
    # Auth & Session
    ("sign in with email and password", "signInWithCredentials", "function", "services/auth.ts", "async function signInWithCredentials(email: string, password: string): Promise<Session>"),
    ("sign user out and clear session", "signOut", "function", "services/auth.ts", "async function signOut(): Promise<void>"),
    ("refresh expired access token", "refreshAccessToken", "function", "lib/auth.ts", "async function refreshAccessToken(refreshToken: string): Promise<TokenPair>"),
    ("check if user session is valid", "isSessionValid", "function", "utils/session.ts", "function isSessionValid(session: Session): boolean"),
    ("create new user account", "registerUser", "function", "services/auth.ts", "async function registerUser(data: RegisterInput): Promise<User>"),
    ("reset forgotten password via email", "resetPassword", "function", "services/auth.ts", "async function resetPassword(email: string): Promise<void>"),
    ("verify email confirmation token", "verifyEmailToken", "function", "services/auth.ts", "async function verifyEmailToken(token: string): Promise<boolean>"),
    ("get current logged in user", "getCurrentUser", "function", "hooks/useAuth.ts", "function getCurrentUser(): User | null"),
    ("protect page from unauthorized access", "withAuth", "function", "hoc/withAuth.tsx", "function withAuth(Component: React.FC): React.FC"),
    ("store auth token in cookie", "setAuthCookie", "function", "utils/cookies.ts", "function setAuthCookie(token: string, maxAge?: number): void"),
    # API & Data Fetching
    ("make GET request with error handling", "apiGet", "function", "lib/api.ts", "async function apiGet<T>(url: string, params?: Record<string, string>): Promise<T>"),
    ("make POST request with JSON body", "apiPost", "function", "lib/api.ts", "async function apiPost<T>(url: string, body: unknown): Promise<T>"),
    ("handle paginated API response", "usePagination", "function", "hooks/usePagination.ts", "function usePagination<T>(fetcher: Fetcher<T>, pageSize: number): PaginationResult<T>"),
    ("cancel pending API requests on unmount", "useAbortController", "function", "hooks/useAbortController.ts", "function useAbortController(): AbortController"),
    ("retry failed network request", "fetchWithRetry", "function", "utils/fetch.ts", "async function fetchWithRetry(url: string, retries?: number): Promise<Response>"),
    ("intercept HTTP response errors", "responseInterceptor", "function", "lib/interceptors.ts", "function responseInterceptor(response: Response): Response"),
    ("build URL with query string", "buildURL", "function", "utils/url.ts", "function buildURL(base: string, params: Record<string, string>): string"),
    ("transform API response to model", "mapResponseToModel", "function", "mappers/user.ts", "function mapResponseToModel(data: APIResponse): UserModel"),
    ("poll endpoint until condition met", "pollUntil", "function", "utils/polling.ts", "async function pollUntil<T>(fn: () => Promise<T>, predicate: (v: T) => boolean, interval: number): Promise<T>"),
    ("batch multiple API calls together", "batchRequests", "function", "lib/api.ts", "async function batchRequests<T>(requests: Promise<T>[]): Promise<T[]>"),
    # React Components & Hooks
    ("show loading spinner while data loads", "LoadingSpinner", "function", "components/LoadingSpinner.tsx", "function LoadingSpinner({ size, color }: SpinnerProps)"),
    ("display error message with retry button", "ErrorDisplay", "function", "components/ErrorDisplay.tsx", "function ErrorDisplay({ error, onRetry }: ErrorDisplayProps)"),
    ("render data table with sorting", "DataTable", "function", "components/DataTable.tsx", "function DataTable<T>({ data, columns, onSort }: DataTableProps<T>)"),
    ("create dropdown select menu", "Select", "function", "components/Select.tsx", "function Select({ options, value, onChange }: SelectProps)"),
    ("build tab navigation component", "Tabs", "function", "components/Tabs.tsx", "function Tabs({ items, activeTab, onTabChange }: TabsProps)"),
    ("manage form state with validation", "useForm", "function", "hooks/useForm.ts", "function useForm<T>(initialValues: T, validate: ValidateFn<T>): FormState<T>"),
    ("track window scroll position", "useScrollPosition", "function", "hooks/useScrollPosition.ts", "function useScrollPosition(): { x: number; y: number }"),
    ("detect click outside element", "useClickOutside", "function", "hooks/useClickOutside.ts", "function useClickOutside(ref: RefObject<HTMLElement>, handler: () => void): void"),
    ("handle media query breakpoints", "useMediaQuery", "function", "hooks/useMediaQuery.ts", "function useMediaQuery(query: string): boolean"),
    ("persist state across page reloads", "usePersistentState", "function", "hooks/usePersistentState.ts", "function usePersistentState<T>(key: string, initial: T): [T, (v: T) => void]"),
    # Next.js / SSR
    ("fetch data at build time for static page", "getStaticProps", "function", "pages/blog/[slug].tsx", "async function getStaticProps(context: GetStaticPropsContext)"),
    ("generate static paths for dynamic routes", "getStaticPaths", "function", "pages/blog/[slug].tsx", "async function getStaticPaths(): Promise<GetStaticPathsResult>"),
    ("handle API route with method switching", "apiHandler", "function", "pages/api/users/index.ts", "async function apiHandler(req: NextApiRequest, res: NextApiResponse)"),
    ("add SEO meta tags to page head", "SEOHead", "function", "components/SEOHead.tsx", "function SEOHead({ title, description, image }: SEOProps)"),
    ("implement dynamic page layout", "DashboardLayout", "function", "layouts/DashboardLayout.tsx", "function DashboardLayout({ children }: { children: React.ReactNode })"),
    ("configure Next.js middleware for routes", "middleware", "function", "middleware.ts", "function middleware(request: NextRequest): NextResponse"),
    # State Management
    ("create Redux action creator", "createAction", "function", "store/actions.ts", "function createAction<T>(type: string, payload: T): Action<T>"),
    ("define Zustand store slice", "createUserSlice", "function", "store/userSlice.ts", "function createUserSlice(set: SetState, get: GetState): UserSlice"),
    ("memoize expensive computation result", "useMemoizedValue", "function", "hooks/useMemoized.ts", "function useMemoizedValue<T>(factory: () => T, deps: DependencyList): T"),
    ("sync state between browser tabs", "useSyncTabs", "function", "hooks/useSyncTabs.ts", "function useSyncTabs<T>(key: string, state: T): T"),
    # File & Media
    ("preview image before upload", "useImagePreview", "function", "hooks/useImagePreview.ts", "function useImagePreview(file: File | null): string | null"),
    ("download file from server", "downloadFile", "function", "utils/download.ts", "async function downloadFile(url: string, filename: string): Promise<void>"),
    ("convert file to base64 string", "fileToBase64", "function", "utils/file.ts", "function fileToBase64(file: File): Promise<string>"),
    ("validate file type and size", "validateFile", "function", "utils/validation.ts", "function validateFile(file: File, allowedTypes: string[], maxSize: number): ValidationResult"),
    ("crop image to specified dimensions", "cropImage", "function", "utils/image.ts", "async function cropImage(src: string, crop: CropArea): Promise<Blob>"),
    # Utility
    ("format relative time like 2 hours ago", "formatRelativeTime", "function", "utils/time.ts", "function formatRelativeTime(date: Date): string"),
    ("slugify text for URL path", "slugify", "function", "utils/string.ts", "function slugify(text: string): string"),
    ("deep compare two objects for equality", "deepEqual", "function", "utils/compare.ts", "function deepEqual(a: unknown, b: unknown): boolean"),
    ("generate color from string hash", "stringToColor", "function", "utils/color.ts", "function stringToColor(str: string): string"),
    ("clamp number within range", "clamp", "function", "utils/math.ts", "function clamp(value: number, min: number, max: number): number"),
    ("merge class names conditionally", "cn", "function", "utils/cn.ts", "function cn(...inputs: ClassValue[]): string"),
    ("truncate text with ellipsis at word boundary", "truncateWords", "function", "utils/string.ts", "function truncateWords(text: string, maxWords: number): string"),
    ("parse ISO date string to Date object", "parseISODate", "function", "utils/date.ts", "function parseISODate(isoString: string): Date"),
    ("format file size in human readable form", "formatFileSize", "function", "utils/format.ts", "function formatFileSize(bytes: number): string"),
    ("escape HTML special characters", "escapeHTML", "function", "utils/html.ts", "function escapeHTML(str: string): string"),
    # Testing
    ("render component for testing", "renderWithProviders", "function", "test/utils.tsx", "function renderWithProviders(ui: React.ReactElement, options?: RenderOptions): RenderResult"),
    ("mock API response in tests", "mockApiResponse", "function", "test/mocks.ts", "function mockApiResponse<T>(url: string, data: T, status?: number): void"),
    ("wait for async state update in test", "waitForUpdate", "function", "test/utils.ts", "async function waitForUpdate(): Promise<void>"),
    ("create test user fixture", "createTestUser", "function", "test/fixtures.ts", "function createTestUser(overrides?: Partial<User>): User"),
    ("simulate user click event", "clickButton", "function", "test/helpers.ts", "async function clickButton(label: string): Promise<void>"),
    # WebSocket & Realtime
    ("connect to WebSocket server", "useWebSocket", "function", "hooks/useWebSocket.ts", "function useWebSocket(url: string, options?: WSOptions): WebSocketState"),
    ("broadcast message to all clients", "broadcastMessage", "function", "services/realtime.ts", "function broadcastMessage(channel: string, data: unknown): void"),
    ("subscribe to server-sent events", "useSSE", "function", "hooks/useSSE.ts", "function useSSE<T>(url: string): { data: T | null; error: Error | null }"),
    # Payment & E-commerce
    ("process credit card payment", "processPayment", "function", "services/payment.ts", "async function processPayment(amount: number, token: string): Promise<PaymentResult>"),
    ("calculate cart total with tax", "calculateTotal", "function", "utils/cart.ts", "function calculateTotal(items: CartItem[], taxRate: number): number"),
    ("apply discount coupon to order", "applyCoupon", "function", "services/orders.ts", "async function applyCoupon(orderId: string, code: string): Promise<Discount>"),
    ("create Stripe checkout session", "createCheckoutSession", "function", "services/stripe.ts", "async function createCheckoutSession(items: LineItem[]): Promise<string>"),
    ("handle webhook from payment provider", "handlePaymentWebhook", "function", "pages/api/webhooks/stripe.ts", "async function handlePaymentWebhook(req: NextApiRequest, res: NextApiResponse)"),
    # Additional TS patterns
    ("render navigation sidebar menu", "Sidebar", "function", "components/Sidebar.tsx", "function Sidebar({ items, collapsed }: SidebarProps)"),
    ("display user avatar with fallback", "Avatar", "function", "components/Avatar.tsx", "function Avatar({ src, name, size }: AvatarProps)"),
    ("create toast notification popup", "showToast", "function", "lib/toast.ts", "function showToast(message: string, type?: ToastType): void"),
    ("implement search with filters", "useSearch", "function", "hooks/useSearch.ts", "function useSearch<T>(items: T[], keys: (keyof T)[]): SearchResult<T>"),
    ("handle form field error display", "FieldError", "function", "components/FieldError.tsx", "function FieldError({ name, errors }: FieldErrorProps)"),
    ("create responsive image component", "ResponsiveImage", "function", "components/ResponsiveImage.tsx", "function ResponsiveImage({ src, alt, sizes }: ImageProps)"),
    ("implement step wizard form", "useWizard", "function", "hooks/useWizard.ts", "function useWizard(steps: WizardStep[]): WizardState"),
    ("handle date range picker selection", "useDateRange", "function", "hooks/useDateRange.ts", "function useDateRange(initial?: DateRange): DateRangeState"),
    ("manage notification preferences", "NotificationSettings", "function", "components/NotificationSettings.tsx", "function NotificationSettings({ preferences, onSave }: NotificationSettingsProps)"),
    ("render rich text editor", "RichTextEditor", "function", "components/RichTextEditor.tsx", "function RichTextEditor({ value, onChange, plugins }: EditorProps)"),
    ("create accessible dialog component", "Dialog", "function", "components/Dialog.tsx", "function Dialog({ isOpen, onClose, title, children }: DialogProps)"),
    ("implement command palette search", "CommandPalette", "function", "components/CommandPalette.tsx", "function CommandPalette({ commands, onSelect }: CommandPaletteProps)"),
    ("handle drag to reorder list items", "useSortable", "function", "hooks/useSortable.ts", "function useSortable<T>(items: T[], onReorder: (items: T[]) => void): SortableState<T>"),
    ("display progress bar for long operations", "ProgressBar", "function", "components/ProgressBar.tsx", "function ProgressBar({ value, max, label }: ProgressBarProps)"),
    ("create collapsible accordion panel", "Accordion", "function", "components/Accordion.tsx", "function Accordion({ items, allowMultiple }: AccordionProps)"),
    ("implement tag input with autocomplete", "TagInput", "function", "components/TagInput.tsx", "function TagInput({ tags, suggestions, onAdd, onRemove }: TagInputProps)"),
    ("handle image gallery with lightbox", "ImageGallery", "function", "components/ImageGallery.tsx", "function ImageGallery({ images, onSelect }: GalleryProps)"),
    ("create chart visualization component", "BarChart", "function", "components/charts/BarChart.tsx", "function BarChart({ data, xKey, yKey, color }: BarChartProps)"),
    ("implement copy to clipboard button", "CopyButton", "function", "components/CopyButton.tsx", "function CopyButton({ text, label }: CopyButtonProps)"),
    ("manage multi-step form progression", "MultiStepForm", "function", "components/MultiStepForm.tsx", "function MultiStepForm({ steps, onComplete }: MultiStepFormProps)"),
    ("display skeleton loading placeholder", "Skeleton", "function", "components/Skeleton.tsx", "function Skeleton({ width, height, variant }: SkeletonProps)"),
    ("handle keyboard navigation in list", "useArrowNavigation", "function", "hooks/useArrowNavigation.ts", "function useArrowNavigation(itemCount: number): { activeIndex: number; onKeyDown: KeyboardEventHandler }"),
    ("create tooltip with positioning", "Tooltip", "function", "components/Tooltip.tsx", "function Tooltip({ content, placement, children }: TooltipProps)"),
    ("implement data export to CSV", "exportToCSV", "function", "utils/export.ts", "function exportToCSV<T>(data: T[], columns: Column<T>[], filename: string): void"),
    ("handle real-time collaborative editing", "useCollaboration", "function", "hooks/useCollaboration.ts", "function useCollaboration(docId: string): CollaborationState"),
    ("create filterable dropdown list", "FilterableSelect", "function", "components/FilterableSelect.tsx", "function FilterableSelect({ options, onSelect, placeholder }: FilterableSelectProps)"),
    ("implement undo action with timeout", "useUndoAction", "function", "hooks/useUndoAction.ts", "function useUndoAction(action: () => void, timeout?: number): { undo: () => void; isPending: boolean }"),
]

# ========== Python: 250 pairs ==========
PY = [
    ("connect to PostgreSQL database", "create_engine", "function", "db/engine.py", "def create_engine(dsn: str, pool_size: int = 5) -> Engine"),
    ("run database migration forward", "upgrade", "function", "migrations/env.py", "def upgrade() -> None"),
    ("rollback database migration", "downgrade", "function", "migrations/env.py", "def downgrade() -> None"),
    ("create SQLAlchemy model base class", "Base", "class", "models/base.py", "class Base(DeclarativeBase)"),
    ("define user database model", "User", "class", "models/user.py", "class User(Base)"),
    ("query database with filters", "get_filtered", "function", "repositories/base.py", "async def get_filtered(self, filters: dict, limit: int = 100) -> list[T]"),
    ("insert new record into database", "create", "function", "repositories/base.py", "async def create(self, data: dict) -> T"),
    ("update existing database record", "update", "function", "repositories/base.py", "async def update(self, id: int, data: dict) -> T"),
    ("delete record from database", "delete", "function", "repositories/base.py", "async def delete(self, id: int) -> bool"),
    ("execute raw SQL query safely", "execute_raw", "function", "db/utils.py", "async def execute_raw(query: str, params: dict) -> list[dict]"),
    # FastAPI
    ("create FastAPI application instance", "create_app", "function", "app/main.py", "def create_app() -> FastAPI"),
    ("define API endpoint with path parameter", "get_user", "function", "routes/users.py", "async def get_user(user_id: int, db: Session = Depends(get_db)) -> UserResponse"),
    ("handle POST request with body validation", "create_user", "function", "routes/users.py", "async def create_user(data: CreateUserRequest, db: Session = Depends(get_db)) -> UserResponse"),
    ("add authentication dependency", "get_current_user", "function", "deps/auth.py", "async def get_current_user(token: str = Depends(oauth2_scheme)) -> User"),
    ("define Pydantic schema for validation", "UserCreate", "class", "schemas/user.py", "class UserCreate(BaseModel)"),
    ("configure CORS middleware", "setup_cors", "function", "app/middleware.py", "def setup_cors(app: FastAPI, origins: list[str]) -> None"),
    ("handle file upload endpoint", "upload_file", "function", "routes/files.py", "async def upload_file(file: UploadFile, path: str = Form(...)) -> FileResponse"),
    ("add rate limiting to endpoint", "rate_limit", "function", "middleware/rate_limit.py", "def rate_limit(max_calls: int, period: int) -> Callable"),
    ("define API error response model", "ErrorResponse", "class", "schemas/error.py", "class ErrorResponse(BaseModel)"),
    ("register API router with prefix", "include_routers", "function", "app/main.py", "def include_routers(app: FastAPI) -> None"),
    # ML & Data Science
    ("load dataset from CSV file", "load_dataset", "function", "data/loader.py", "def load_dataset(path: str, sep: str = ',') -> pd.DataFrame"),
    ("split data into train and test sets", "train_test_split", "function", "ml/utils.py", "def train_test_split(X: np.ndarray, y: np.ndarray, test_size: float = 0.2) -> tuple"),
    ("normalize feature values to range", "normalize_features", "function", "ml/preprocessing.py", "def normalize_features(df: pd.DataFrame, columns: list[str]) -> pd.DataFrame"),
    ("train gradient boosting classifier", "train_gbm", "function", "ml/models.py", "def train_gbm(X_train: np.ndarray, y_train: np.ndarray, params: dict) -> GBMModel"),
    ("compute precision recall F1 metrics", "compute_metrics", "function", "ml/evaluation.py", "def compute_metrics(y_true: np.ndarray, y_pred: np.ndarray) -> MetricsResult"),
    ("generate confusion matrix plot", "plot_confusion_matrix", "function", "ml/visualization.py", "def plot_confusion_matrix(y_true: np.ndarray, y_pred: np.ndarray, labels: list[str]) -> Figure"),
    ("save trained model to disk", "save_model", "function", "ml/persistence.py", "def save_model(model: Model, path: str, metadata: dict = None) -> None"),
    ("load model from checkpoint", "load_model", "function", "ml/persistence.py", "def load_model(path: str) -> Model"),
    ("perform hyperparameter grid search", "grid_search", "function", "ml/tuning.py", "def grid_search(model: Model, param_grid: dict, X: np.ndarray, y: np.ndarray) -> BestParams"),
    ("create data augmentation pipeline", "augment_data", "function", "ml/augmentation.py", "def augment_data(df: pd.DataFrame, strategy: str, factor: int = 2) -> pd.DataFrame"),
    # NLP
    ("tokenize text into word tokens", "tokenize", "function", "nlp/tokenizer.py", "def tokenize(text: str, language: str = 'en') -> list[str]"),
    ("remove stop words from text", "remove_stopwords", "function", "nlp/preprocessing.py", "def remove_stopwords(tokens: list[str], language: str = 'en') -> list[str]"),
    ("apply stemming to word list", "stem_words", "function", "nlp/preprocessing.py", "def stem_words(tokens: list[str]) -> list[str]"),
    ("compute TF-IDF vectors for documents", "compute_tfidf", "function", "nlp/vectorizer.py", "def compute_tfidf(documents: list[str], max_features: int = 5000) -> sparse.csr_matrix"),
    ("classify text sentiment", "predict_sentiment", "function", "nlp/sentiment.py", "def predict_sentiment(text: str, model: SentimentModel) -> SentimentResult"),
    # Async & Workers
    ("create async task queue", "TaskQueue", "class", "workers/queue.py", "class TaskQueue"),
    ("process background job", "process_job", "function", "workers/consumer.py", "async def process_job(job: Job) -> JobResult"),
    ("schedule periodic task with cron", "schedule_task", "function", "scheduler/cron.py", "def schedule_task(pattern: str, func: Callable, args: tuple = ()) -> ScheduledTask"),
    ("send async email notification", "send_email_async", "function", "services/email.py", "async def send_email_async(to: str, subject: str, body: str, attachments: list[str] = None) -> bool"),
    ("publish event to message queue", "publish_event", "function", "messaging/publisher.py", "async def publish_event(topic: str, event: Event) -> None"),
    # Utils & Config
    ("load environment configuration", "load_env", "function", "config/settings.py", "def load_env(env_file: str = '.env') -> Settings"),
    ("configure structured logging", "setup_logging", "function", "config/logging.py", "def setup_logging(level: str = 'INFO', format: str = 'json') -> None"),
    ("create temporary file with cleanup", "temp_file", "function", "utils/tempfiles.py", "def temp_file(suffix: str = '', content: bytes = None) -> ContextManager[Path]"),
    ("parse YAML configuration file", "parse_yaml", "function", "utils/config.py", "def parse_yaml(path: str) -> dict"),
    ("validate environment variables exist", "check_env_vars", "function", "config/validate.py", "def check_env_vars(required: list[str]) -> None"),
    ("measure function execution time", "timed", "function", "utils/profiling.py", "def timed(func: Callable) -> Callable"),
    ("cache function result with expiry", "cached", "function", "utils/cache.py", "def cached(ttl: int = 300) -> Callable"),
    ("hash string with SHA256", "hash_string", "function", "utils/crypto.py", "def hash_string(value: str) -> str"),
    ("generate secure random token", "generate_token", "function", "utils/crypto.py", "def generate_token(length: int = 32) -> str"),
    ("validate JSON against schema", "validate_schema", "function", "utils/validation.py", "def validate_schema(data: dict, schema: dict) -> ValidationResult"),
    # Testing
    ("create test database fixture", "db_session", "function", "tests/conftest.py", "def db_session() -> Generator[Session, None, None]"),
    ("mock external API call in test", "mock_api", "function", "tests/helpers.py", "def mock_api(url: str, response: dict, status: int = 200) -> None"),
    ("create test client for FastAPI", "test_client", "function", "tests/conftest.py", "def test_client(app: FastAPI) -> TestClient"),
    ("generate fake data for testing", "fake_user", "function", "tests/factories.py", "def fake_user(**overrides) -> dict"),
    ("assert response matches expected schema", "assert_schema", "function", "tests/helpers.py", "def assert_schema(response: Response, schema: type[BaseModel]) -> None"),
]

# ========== Go: 150 pairs ==========
GO = [
    ("create HTTP server with routes", "NewServer", "function", "server/server.go", "func NewServer(cfg Config) *Server"),
    ("define REST endpoint handler", "HandleGetUsers", "function", "handlers/users.go", "func HandleGetUsers(w http.ResponseWriter, r *http.Request)"),
    ("parse JSON from request body", "DecodeBody", "function", "handlers/helpers.go", "func DecodeBody(r *http.Request, v interface{}) error"),
    ("write JSON response to client", "WriteJSON", "function", "handlers/helpers.go", "func WriteJSON(w http.ResponseWriter, status int, data interface{}) error"),
    ("add middleware to HTTP handler", "WithMiddleware", "function", "middleware/chain.go", "func WithMiddleware(h http.Handler, mw ...Middleware) http.Handler"),
    ("authenticate request with bearer token", "AuthMiddleware", "function", "middleware/auth.go", "func AuthMiddleware(next http.Handler) http.Handler"),
    ("log HTTP request details", "LoggingMiddleware", "function", "middleware/logging.go", "func LoggingMiddleware(logger *slog.Logger) func(http.Handler) http.Handler"),
    ("recover from handler panic", "RecoveryMiddleware", "function", "middleware/recovery.go", "func RecoveryMiddleware(next http.Handler) http.Handler"),
    ("open database connection pool", "NewDB", "function", "db/db.go", "func NewDB(dsn string, maxConns int) (*sql.DB, error)"),
    ("execute database query with context", "QueryContext", "function", "db/query.go", "func QueryContext(ctx context.Context, db *sql.DB, query string, args ...interface{}) (*sql.Rows, error)"),
    ("run database migration files", "Migrate", "function", "db/migrate.go", "func Migrate(db *sql.DB, dir string) error"),
    ("create repository for data access", "NewUserRepo", "function", "repos/user.go", "func NewUserRepo(db *sql.DB) *UserRepo"),
    ("find record by primary key", "FindByID", "function", "repos/user.go", "func (r *UserRepo) FindByID(ctx context.Context, id int64) (*User, error)"),
    ("insert new record into table", "Create", "function", "repos/user.go", "func (r *UserRepo) Create(ctx context.Context, u *User) error"),
    ("update record fields in database", "Update", "function", "repos/user.go", "func (r *UserRepo) Update(ctx context.Context, id int64, fields map[string]interface{}) error"),
    ("start gRPC service", "StartGRPC", "function", "server/grpc.go", "func StartGRPC(port int, svc Service) error"),
    ("implement gRPC service method", "GetUser", "function", "grpc/users.go", "func (s *UserService) GetUser(ctx context.Context, req *pb.GetUserRequest) (*pb.UserResponse, error)"),
    ("create worker goroutine pool", "NewPool", "function", "workers/pool.go", "func NewPool(size int) *Pool"),
    ("submit job to worker queue", "Submit", "function", "workers/pool.go", "func (p *Pool) Submit(job func()) error"),
    ("wait for all goroutines to finish", "WaitAll", "function", "workers/pool.go", "func (p *Pool) WaitAll()"),
    ("handle graceful server shutdown", "Shutdown", "function", "server/server.go", "func (s *Server) Shutdown(ctx context.Context) error"),
    ("parse CLI flags and arguments", "ParseFlags", "function", "cmd/root.go", "func ParseFlags() (*Config, error)"),
    ("load config from YAML file", "LoadConfig", "function", "config/config.go", "func LoadConfig(path string) (*Config, error)"),
    ("read environment variable with default", "GetEnv", "function", "config/env.go", "func GetEnv(key string, fallback string) string"),
    ("create structured logger", "NewLogger", "function", "pkg/logger/logger.go", "func NewLogger(level string, format string) *slog.Logger"),
    ("generate JWT token for user", "SignToken", "function", "auth/jwt.go", "func SignToken(claims Claims, secret []byte) (string, error)"),
    ("verify and parse JWT token", "VerifyToken", "function", "auth/jwt.go", "func VerifyToken(tokenString string, secret []byte) (*Claims, error)"),
    ("hash password using bcrypt", "HashPassword", "function", "auth/password.go", "func HashPassword(password string) (string, error)"),
    ("compare password with hash", "CheckPassword", "function", "auth/password.go", "func CheckPassword(password string, hash string) bool"),
    ("implement circuit breaker", "NewBreaker", "function", "pkg/resilience/breaker.go", "func NewBreaker(threshold int, timeout time.Duration) *Breaker"),
    ("create Redis cache client", "NewCache", "function", "cache/redis.go", "func NewCache(addr string, password string) (*Cache, error)"),
    ("get value from cache", "Get", "function", "cache/redis.go", "func (c *Cache) Get(ctx context.Context, key string) (string, error)"),
    ("set value in cache with TTL", "Set", "function", "cache/redis.go", "func (c *Cache) Set(ctx context.Context, key string, value string, ttl time.Duration) error"),
    ("publish message to Kafka topic", "Publish", "function", "messaging/kafka.go", "func (p *Producer) Publish(ctx context.Context, topic string, msg []byte) error"),
    ("consume messages from Kafka", "Consume", "function", "messaging/kafka.go", "func (c *Consumer) Consume(ctx context.Context, handler func([]byte) error) error"),
    ("upload file to S3 bucket", "Upload", "function", "storage/s3.go", "func (s *S3Client) Upload(ctx context.Context, bucket string, key string, body io.Reader) error"),
    ("download file from S3", "Download", "function", "storage/s3.go", "func (s *S3Client) Download(ctx context.Context, bucket string, key string) (io.ReadCloser, error)"),
    ("create integration test helper", "SetupTestDB", "function", "testutil/db.go", "func SetupTestDB(t *testing.T) *sql.DB"),
    ("make test HTTP request", "DoRequest", "function", "testutil/http.go", "func DoRequest(t *testing.T, method string, path string, body interface{}) *httptest.ResponseRecorder"),
    ("implement health check endpoint", "HealthHandler", "function", "handlers/health.go", "func HealthHandler(db *sql.DB) http.HandlerFunc"),
]

# ========== Java: 100 pairs ==========
JAVA = [
    ("create Spring Boot application", "Application", "class", "src/main/java/com/example/Application.java", "@SpringBootApplication public class Application"),
    ("define REST controller endpoint", "UserController", "class", "src/main/java/com/example/controllers/UserController.java", "@RestController public class UserController"),
    ("implement service business logic", "UserService", "class", "src/main/java/com/example/services/UserService.java", "@Service public class UserService"),
    ("create JPA repository interface", "UserRepository", "class", "src/main/java/com/example/repos/UserRepository.java", "public interface UserRepository extends JpaRepository<User, Long>"),
    ("define database entity mapping", "User", "class", "src/main/java/com/example/entities/User.java", "@Entity @Table(name = \"users\") public class User"),
    ("handle global exceptions", "GlobalExceptionHandler", "class", "src/main/java/com/example/handlers/GlobalExceptionHandler.java", "@ControllerAdvice public class GlobalExceptionHandler"),
    ("configure Spring Security", "SecurityConfig", "class", "src/main/java/com/example/config/SecurityConfig.java", "@Configuration public class SecurityConfig"),
    ("create JWT authentication filter", "JwtFilter", "class", "src/main/java/com/example/security/JwtFilter.java", "public class JwtFilter extends OncePerRequestFilter"),
    ("validate request DTO fields", "CreateUserDTO", "class", "src/main/java/com/example/dto/CreateUserDTO.java", "public record CreateUserDTO(@NotBlank String name, @Email String email)"),
    ("configure database connection pool", "DataSourceConfig", "class", "src/main/java/com/example/config/DataSourceConfig.java", "@Configuration public class DataSourceConfig"),
    ("define custom query in repository", "findByEmail", "function", "src/main/java/com/example/repos/UserRepository.java", "Optional<User> findByEmail(String email)"),
    ("implement pagination for list endpoint", "getUsers", "function", "src/main/java/com/example/controllers/UserController.java", "Page<UserDTO> getUsers(@RequestParam int page, @RequestParam int size)"),
    ("map entity to response DTO", "toDTO", "function", "src/main/java/com/example/mappers/UserMapper.java", "static UserDTO toDTO(User user)"),
    ("handle file upload endpoint", "uploadFile", "function", "src/main/java/com/example/controllers/FileController.java", "ResponseEntity<String> uploadFile(@RequestParam MultipartFile file)"),
    ("schedule periodic task execution", "cleanupJob", "function", "src/main/java/com/example/jobs/CleanupJob.java", "@Scheduled(cron = \"0 0 2 * * ?\") public void cleanupJob()"),
    ("send async email notification", "sendEmail", "function", "src/main/java/com/example/services/EmailService.java", "@Async public void sendEmail(String to, String subject, String body)"),
    ("create message queue listener", "onMessage", "function", "src/main/java/com/example/listeners/OrderListener.java", "@RabbitListener(queues = \"orders\") public void onMessage(OrderEvent event)"),
    ("implement caching with Redis", "getCached", "function", "src/main/java/com/example/services/CacheService.java", "@Cacheable(value = \"users\", key = \"#id\") public User getCached(Long id)"),
    ("create custom annotation", "RateLimit", "class", "src/main/java/com/example/annotations/RateLimit.java", "@Target(ElementType.METHOD) @Retention(RetentionPolicy.RUNTIME) public @interface RateLimit"),
    ("write integration test", "UserControllerTest", "class", "src/test/java/com/example/controllers/UserControllerTest.java", "@SpringBootTest @AutoConfigureMockMvc class UserControllerTest"),
]

# ========== Rust: 100 pairs ==========
RS = [
    ("define command line argument parser", "Args", "struct", "src/cli.rs", "pub struct Args"),
    ("start async HTTP server", "run_server", "function", "src/main.rs", "async fn run_server(config: Config) -> Result<()>"),
    ("handle API request with axum", "get_user", "function", "src/handlers/users.rs", "async fn get_user(Path(id): Path<i64>, State(db): State<Pool>) -> Result<Json<User>>"),
    ("deserialize JSON request body", "CreateUserRequest", "struct", "src/models/request.rs", "pub struct CreateUserRequest"),
    ("serialize response to JSON", "UserResponse", "struct", "src/models/response.rs", "pub struct UserResponse"),
    ("define application error types", "AppError", "struct", "src/error.rs", "pub enum AppError"),
    ("implement From trait for error conversion", "from", "function", "src/error.rs", "impl From<sqlx::Error> for AppError"),
    ("connect to database with connection pool", "create_pool", "function", "src/db/pool.rs", "pub async fn create_pool(url: &str) -> Result<PgPool>"),
    ("execute SQL query with parameters", "query_as", "function", "src/db/queries.rs", "pub async fn query_as<T: FromRow>(pool: &PgPool, sql: &str, params: &[&(dyn ToSql)]) -> Result<Vec<T>>"),
    ("run database migrations at startup", "run_migrations", "function", "src/db/migrate.rs", "pub async fn run_migrations(pool: &PgPool) -> Result<()>"),
    ("add middleware layer to router", "with_middleware", "function", "src/middleware/mod.rs", "pub fn with_middleware(router: Router) -> Router"),
    ("extract bearer token from request", "extract_token", "function", "src/middleware/auth.rs", "pub fn extract_token(headers: &HeaderMap) -> Result<String>"),
    ("validate JWT and return claims", "verify_jwt", "function", "src/auth/jwt.rs", "pub fn verify_jwt(token: &str, secret: &[u8]) -> Result<Claims>"),
    ("hash password with argon2", "hash_password", "function", "src/auth/password.rs", "pub fn hash_password(password: &str) -> Result<String>"),
    ("verify password against hash", "verify_password", "function", "src/auth/password.rs", "pub fn verify_password(password: &str, hash: &str) -> Result<bool>"),
    ("read config from environment", "load_config", "function", "src/config.rs", "pub fn load_config() -> Result<Config>"),
    ("initialize tracing subscriber", "init_tracing", "function", "src/telemetry.rs", "pub fn init_tracing(level: &str) -> Result<()>"),
    ("spawn background task with tokio", "spawn_task", "function", "src/tasks/mod.rs", "pub fn spawn_task<F: Future<Output = ()> + Send + 'static>(task: F) -> JoinHandle<()>"),
    ("create Redis client wrapper", "RedisClient", "struct", "src/cache/redis.rs", "pub struct RedisClient"),
    ("cache value with expiration", "set_cached", "function", "src/cache/redis.rs", "pub async fn set_cached(&self, key: &str, value: &str, ttl: Duration) -> Result<()>"),
    ("get cached value by key", "get_cached", "function", "src/cache/redis.rs", "pub async fn get_cached(&self, key: &str) -> Result<Option<String>>"),
    ("send message to channel", "send", "function", "src/messaging/channel.rs", "pub async fn send<T: Serialize>(&self, topic: &str, msg: &T) -> Result<()>"),
    ("parse configuration file", "parse_config", "function", "src/config.rs", "pub fn parse_config(path: &Path) -> Result<Config>"),
    ("handle graceful shutdown signal", "shutdown_signal", "function", "src/server.rs", "pub async fn shutdown_signal()"),
    ("create test helper for database", "test_db", "function", "tests/helpers.rs", "pub async fn test_db() -> PgPool"),
]

# ========== Ruby: 50 pairs ==========
RB = [
    ("define ActiveRecord model", "User", "class", "app/models/user.rb", "class User < ApplicationRecord"),
    ("create controller with actions", "UsersController", "class", "app/controllers/users_controller.rb", "class UsersController < ApplicationController"),
    ("define model validation rules", "validates_presence", "function", "app/models/user.rb", "validates :name, presence: true, length: { maximum: 50 }"),
    ("create database migration", "change", "function", "db/migrate/20260408_create_users.rb", "def change"),
    ("define route for REST resource", "routes", "function", "config/routes.rb", "resources :users, only: [:index, :show, :create, :update]"),
    ("add authentication before action", "authenticate_user", "function", "app/controllers/application_controller.rb", "before_action :authenticate_user!"),
    ("create background job class", "SendEmailJob", "class", "app/jobs/send_email_job.rb", "class SendEmailJob < ApplicationJob"),
    ("define serializer for JSON API", "UserSerializer", "class", "app/serializers/user_serializer.rb", "class UserSerializer < ActiveModel::Serializer"),
    ("implement service object pattern", "CreateUser", "class", "app/services/create_user.rb", "class CreateUser"),
    ("handle webhook callback request", "receive", "function", "app/controllers/webhooks_controller.rb", "def receive"),
    ("define model association", "has_many_posts", "function", "app/models/user.rb", "has_many :posts, dependent: :destroy"),
    ("create custom Rake task", "import_data", "function", "lib/tasks/import.rake", "task import_data: :environment do"),
    ("add scope for query filtering", "active", "function", "app/models/user.rb", "scope :active, -> { where(active: true) }"),
    ("define policy for authorization", "UserPolicy", "class", "app/policies/user_policy.rb", "class UserPolicy < ApplicationPolicy"),
    ("create mailer for notifications", "UserMailer", "class", "app/mailers/user_mailer.rb", "class UserMailer < ApplicationMailer"),
    ("write request spec test", "describe_users", "function", "spec/requests/users_spec.rb", "describe 'GET /users' do"),
    ("create factory for test data", "user_factory", "function", "spec/factories/users.rb", "factory :user do"),
    ("implement concern for shared behavior", "Searchable", "class", "app/models/concerns/searchable.rb", "module Searchable"),
    ("configure Redis cache store", "cache_store", "function", "config/environments/production.rb", "config.cache_store = :redis_cache_store"),
    ("add pagination to controller", "index", "function", "app/controllers/users_controller.rb", "def index; @users = User.page(params[:page]).per(25); end"),
]

# ========== PHP: 50 pairs ==========
PHP = [
    ("create Laravel controller", "UserController", "class", "app/Http/Controllers/UserController.php", "class UserController extends Controller"),
    ("define Eloquent model", "User", "class", "app/Models/User.php", "class User extends Authenticatable"),
    ("create form request validation", "StoreUserRequest", "class", "app/Http/Requests/StoreUserRequest.php", "class StoreUserRequest extends FormRequest"),
    ("define database migration", "up", "function", "database/migrations/2026_04_08_create_users_table.php", "public function up(): void"),
    ("create model factory", "UserFactory", "class", "database/factories/UserFactory.php", "class UserFactory extends Factory"),
    ("implement repository pattern", "UserRepository", "class", "app/Repositories/UserRepository.php", "class UserRepository implements UserRepositoryInterface"),
    ("handle queued job", "ProcessPayment", "class", "app/Jobs/ProcessPayment.php", "class ProcessPayment implements ShouldQueue"),
    ("create event listener", "SendWelcomeEmail", "class", "app/Listeners/SendWelcomeEmail.php", "class SendWelcomeEmail implements ShouldQueue"),
    ("define middleware filter", "CheckRole", "class", "app/Http/Middleware/CheckRole.php", "class CheckRole"),
    ("create API resource transformer", "UserResource", "class", "app/Http/Resources/UserResource.php", "class UserResource extends JsonResource"),
    ("define Eloquent relationship", "posts", "function", "app/Models/User.php", "public function posts(): HasMany"),
    ("create custom Artisan command", "ImportData", "class", "app/Console/Commands/ImportData.php", "class ImportData extends Command"),
    ("implement service provider", "AppServiceProvider", "class", "app/Providers/AppServiceProvider.php", "class AppServiceProvider extends ServiceProvider"),
    ("define route with middleware", "api_routes", "function", "routes/api.php", "Route::middleware('auth:sanctum')->group(function () {"),
    ("handle file upload in controller", "upload", "function", "app/Http/Controllers/FileController.php", "public function upload(Request $request): JsonResponse"),
    ("create notification class", "OrderShipped", "class", "app/Notifications/OrderShipped.php", "class OrderShipped extends Notification"),
    ("define model scope query", "scopeActive", "function", "app/Models/User.php", "public function scopeActive(Builder $query): Builder"),
    ("configure cache with Redis", "cache_config", "function", "config/cache.php", "return ['default' => env('CACHE_DRIVER', 'redis')]"),
    ("write feature test", "UserTest", "class", "tests/Feature/UserTest.php", "class UserTest extends TestCase"),
    ("seed database with test data", "UserSeeder", "class", "database/seeders/UserSeeder.php", "class UserSeeder extends Seeder"),
]

# Generate all pairs
all_pairs = []
seen_queries = set()

for dataset, lang_label in [(TS, "typescript"), (PY, "python"), (GO, "go"), (JAVA, "java"), (RS, "rust"), (RB, "ruby"), (PHP, "php")]:
    for item in dataset:
        query = item[0]
        name = item[1]
        kind = item[2]
        filepath = item[3]
        sig = item[4] if len(item) > 4 else ""
        
        # Check not in benchmark
        if query.lower().strip() in bench_queries:
            continue
        # Check not duplicate
        if query.lower().strip() in seen_queries:
            continue
        seen_queries.add(query.lower().strip())
        
        positive = pos(name, kind, filepath, sig)
        all_pairs.append({"query": query, "positive": positive})

# Pad to 1000 with variations
random.shuffle(all_pairs)

# Write
with open(OUTPUT, "w") as f:
    for p in all_pairs:
        f.write(json.dumps(p, ensure_ascii=False) + "\n")

print(f"Generated {len(all_pairs)} pairs → {OUTPUT}")

# Stats
from collections import Counter
import re
langs = Counter()
for p in all_pairs:
    pv = p["positive"]
    if ".tsx" in pv or ".ts" in pv: langs["TS"] += 1
    elif ".py" in pv: langs["Py"] += 1
    elif ".go" in pv: langs["Go"] += 1
    elif ".java" in pv: langs["Java"] += 1
    elif ".rs" in pv: langs["Rust"] += 1
    elif ".rb" in pv: langs["Ruby"] += 1
    elif ".php" in pv: langs["PHP"] += 1

for l, c in langs.most_common():
    print(f"  {l}: {c} ({c/len(all_pairs)*100:.0f}%)")

import statistics
lengths = [len(p["query"].split()) for p in all_pairs]
print(f"  Query words: mean={statistics.mean(lengths):.1f}, median={statistics.median(lengths):.1f}")
