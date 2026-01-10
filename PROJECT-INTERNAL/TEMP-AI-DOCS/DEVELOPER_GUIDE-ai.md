# Developer Onboarding Guide

**`[AI Assistant Instruction: Purpose and Maintenance of This Document]`**

> `(1) **Purpose:** Your purpose is to maintain this DEVELOPER_GUIDE.md as the primary onboarding document for *human developers*. The language and content must be clear, direct, and focused on helping a human understand the project's architecture and workflow.`
>
> `(2) **Structure Mandate:** You must preserve the existing three-part structure of this document: 1. Onboarding Protocol, 2. System Architecture, and 3. Development Workflow. Do not alter these main sections.`
>
> `(3) **Content Generation Mandate:** Your primary task is to replace the `[AI-INSTRUCTIONS: ...]` placeholders with detailed, project-specific information relevant to a human developer. Your goal is to create a complete, standalone guide.`
>
> `(4) **Maintenance Rule:** Update the content within the placeholders only when the fundamental architecture, technology stack, or core development processes change. This document must always reflect the current state of the repository.`

---

## 1. Onboarding Protocol: Your First Hour

Welcome. This guide provides the high-level overview necessary to understand the project's structure and begin contributing effectively.

#### **Step 1: Environment Setup**
The first step is to get the project running on your local machine.

`[AI-INSTRUCTIONS: Detail the prerequisites from the main README.md. Provide the exact, step-by-step commands a developer needs to clone the repository, install all necessary dependencies (e.g., 'pnpm install', 'go mod tidy'), set up environment variables (from '.env.example'), and run the project for the first time in development mode (e.g., 'pnpm dev' or 'go run ./cmd/server').]`

#### **Step 2: Understand the Project's "Brain" - The `.PROJECT-INTERNAL` Directory**
All project-specific documentation, rules, tasks, and operational protocols reside in the `.PROJECT-INTERNAL/` directory. This is the **single source of truth** for the project's execution. Before writing any code, you must familiarize yourself with its contents.

#### **Step 3: Read the Core Documents (In This Order)**
To understand the rules of this repository, read the following documents from `.PROJECT-INTERNAL/`:

1.  **`AI-INSTRUCTIONS.md`**: This is **"The Constitution"**. It defines the formal procedures that govern development, especially the interaction between you (the Operator) and the AI Assistant. It dictates the "how" of our work.
2.  **`PROJECT-DEVELOPMENT-ELABORATION.md`**: This is the official **backlog and development plan**. It details what has been completed, what is in progress, and defines the next specific tasks. It dictates the "what" and "when" of our work.
3.  **`DEVELOPMENT-PRACTICES.md`**: This is the project's **living knowledge base**. It starts empty and is populated with solutions to recurring technical challenges to prevent regressions and save time.

---

## 2. System Architecture & Design Philosophy

This section describes the technical architecture and the core principles behind it.

#### **Design Philosophy**
The project's design prioritizes clarity, maintainability, and traceability over premature optimization. We follow a "documentation-first" approach where planning is a formal, recorded activity.

#### **Directory Structure**
The repository is organized to clearly separate concerns.

`[AI-INSTRUCTIONS: Provide a high-level breakdown of the project's directory structure using a tree format. Explain the purpose of key folders, for example: 'src/' for main application source, 'cmd/' for executable entry points, 'internal/' for non-reusable project code, 'pkg/' for reusable libraries, 'scripts/' for automation, and 'web/' for frontend assets.]`

#### **Architectural Decision Records (ADRs)**
Significant architectural decisions, their context, and their consequences are formally documented in Architecture Decision Records (ADRs). This practice ensures that the reasoning behind our technical choices is preserved and accessible.

All ADRs are located in the `PROJECT-INTERNAL/ARCHITECTURE/` directory. When making a new architectural decision, use the `adr-template.md` as a starting point.

#### **Data Flow / Core Logic**
Understanding how data moves through the system is key to understanding the application.

`[AI-INSTRUCTIONS: Describe the main data flow or request lifecycle. For a web API, explain what happens from the moment a user's HTTP request hits the server to when a response is sent. For a data processing daemon, explain the event loop or the steps in the processing pipeline. Mention key modules or functions involved in this process.]`

#### **Key Technologies & Libraries**
The project is built upon a specific set of technologies chosen for their robustness and performance.

`[AI-INSTRUCTIONS: List the primary technologies, frameworks, and significant libraries used in the project (e.g., Backend: Go with the Echo framework; Database: PostgreSQL with the sqlx library; Frontend: React with Vite). Briefly state the role of each component.]`

---

## 3. Development Workflow & Contribution

Your day-to-day contributions must follow these standardized procedures to ensure code quality and project consistency.

#### **Branching and Commit Strategy**
We use a structured branching model to keep the main branch stable.

`[AI-INSTRUCTIONS: Specify the required branching strategy (e.g., "All work must be done in feature branches named 'feature/[ticket-id]-[short-description]'"). Also, define the required commit message format (e.g., "We follow the Conventional Commits specification: 'feat: add user authentication endpoint'").]`

#### **Testing Protocol**
All new code must be accompanied by tests.

`[AI-INSTRUCTIONS: Explain the project's testing strategy. Specify where tests are located (e.g., alongside the source code in '_test.go' files). Provide the exact command used to run the entire test suite (e.g., 'go test -v ./...'). State any requirements for code coverage.]`

#### **Working with the AI Assistant**
While you can code independently, the standard workflow for this repository involves collaborating with the AI Assistant to ensure adherence to protocols. The cycle is simple:

1.  **Receive a Work Order:** The AI will issue a formal Work Order based on a task from the `PROJECT-DEVELOPMENT-ELABORATION.md`.
2.  **Execute the Work:** You apply the code, run the commands, and perform the task as specified.
3.  **Verify and Report:** You run the necessary tests and report the outcome (success or a verbatim error log) back to the AI. This loop ensures every step is documented and validated.

#### **How to Propose a New Feature or Change**
Architectural changes and new features must be formally integrated into the plan **before** implementation.

1.  **Do Not Start Coding:** Directly implementing an unplanned feature is a violation of the protocol.
2.  **Initiate a Revision:** Following the procedure in `AI-INSTRUCTIONS.md`, instruct the AI to begin a "Revision Phase" for the `PROJECT-DEVELOPMENT-ELABORATION.md` document.
3.  **Update The Plan:** Work with the AI to formally add the new feature, breaking it down into phases and tasks.
4.  **Formalize the Decision:** If the change involves a significant architectural decision (e.g., adding a new database, changing a core library), it **must be documented** in an **Architecture Decision Record (ADR)**. Use the template located at `PROJECT-INTERNAL/ARCHITECTURE/adr-template.md`.
5.  **Begin Development:** Once the plan is updated and the decision is recorded, development can commence by following the standard Work Order cycle.
